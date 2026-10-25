// Meeting-notes bridge: EventKit calendar, Voice Processing I/O microphone,
// Core Audio process tap (system audio) and user notifications.
//
// Compiled by `build_meeting_bridge()` in build.rs into a static library and
// called from `src/meetings/native/macos.rs` through `meeting_bridge.h`.
// The deployment target is the app's minimum (10.15 / 11.0 on arm64), so every
// API newer than that is behind `#available`; the matching symbols are
// weak-imported by the compiler. Language mode is Swift 5 (no strict
// concurrency); thread safety is handled explicitly with locks and queues.

import AVFoundation
import AppKit
import ApplicationServices
import AudioToolbox
import CoreAudio
import Darwin
import EventKit
import Foundation
import UserNotifications

// MARK: - Helpers

private struct BridgeError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

/// Runs `body`, turning an Objective-C exception (which Swift cannot catch and
/// would abort the app) into a thrown `BridgeError`.
private func objcTry(_ what: String, _ body: () -> Void) throws {
    var reason: UnsafeMutablePointer<CChar>?
    if guidy_mb_objc_try(body, &reason) != 0 {
        let text = reason.map { String(cString: $0) } ?? "Objective-C exception"
        free(reason)
        throw BridgeError("\(what) raised \(text)")
    }
}

private func setError(_ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, _ message: String) {
    guard let out = out else { return }
    out.pointee = strdup(message)
}

/// A value written from a completion handler and read after a semaphore wait.
private final class Box<T> {
    var value: T
    init(_ value: T) { self.value = value }
}

/// True when the process runs from an app bundle (has a bundle identifier).
private func isBundled() -> Bool {
    return Bundle.main.bundleIdentifier != nil
}

private func hasUsageDescription(_ key: String) -> Bool {
    return Bundle.main.object(forInfoDictionaryKey: key) != nil
}

@_cdecl("guidy_mb_free_string")
public func guidyMbFreeString(_ s: UnsafeMutablePointer<CChar>?) {
    free(s)
}

@_cdecl("guidy_mb_is_supported")
public func guidyMbIsSupported() -> Int32 {
    if #available(macOS 14.2, *) {
        return 1
    }
    return 0
}

// MARK: - Calendar (EventKit)

@_cdecl("guidy_mb_calendar_auth_status")
public func guidyMbCalendarAuthStatus() -> Int32 {
    // Raw values: 0 notDetermined, 1 restricted, 2 denied,
    // 3 authorized / fullAccess (14+), 4 writeOnly (14+). They match the
    // GUIDY_MB_AUTH_* constants one to one.
    let raw = EKEventStore.authorizationStatus(for: .event).rawValue
    switch raw {
    case 0...4: return Int32(raw)
    default: return GUIDY_MB_AUTH_UNKNOWN
    }
}

@_cdecl("guidy_mb_request_calendar_access")
public func guidyMbRequestCalendarAccess() -> Int32 {
    // A bundled app without the usage string would be killed by TCC. An
    // unbundled binary (cargo run / cargo test) is attributed to its
    // responsible process (the terminal), which carries its own strings.
    if isBundled() {
        let key: String
        if #available(macOS 14.0, *) {
            key = "NSCalendarsFullAccessUsageDescription"
        } else {
            key = "NSCalendarsUsageDescription"
        }
        if !hasUsageDescription(key) {
            NSLog("[guidy meetings] Info.plist lacks %@; not requesting calendar access", key)
            return 0
        }
    }

    let store = EKEventStore()
    let semaphore = DispatchSemaphore(value: 0)
    let granted = Box(false)
    let completion: (Bool, Error?) -> Void = { ok, error in
        if let error = error {
            NSLog("[guidy meetings] calendar access request failed: %@", "\(error)")
        }
        granted.value = ok
        semaphore.signal()
    }
    if #available(macOS 14.0, *) {
        store.requestFullAccessToEvents(completion: completion)
    } else {
        store.requestAccess(to: .event, completion: completion)
    }
    semaphore.wait()
    return granted.value ? 1 : 0
}

@_cdecl("guidy_mb_events_between")
public func guidyMbEventsBetween(
    _ startMs: Int64,
    _ endMs: Int64,
    _ errorOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutablePointer<CChar>? {
    if guidyMbCalendarAuthStatus() != GUIDY_MB_AUTH_GRANTED {
        setError(errorOut, "calendar access not granted")
        return nil
    }
    guard endMs > startMs else {
        return strdup("[]")
    }

    // A fresh store sees calendars added since the last call and picks up a
    // grant made after process start.
    let store = EKEventStore()
    let start = Date(timeIntervalSince1970: Double(startMs) / 1000.0)
    let end = Date(timeIntervalSince1970: Double(endMs) / 1000.0)
    let predicate = store.predicateForEvents(withStart: start, end: end, calendars: nil)
    let events = store.events(matching: predicate)

    var out: [[String: Any]] = []
    out.reserveCapacity(events.count)
    for event in events {
        var attendees: [[String: Any]] = []
        for participant in event.attendees ?? [] {
            attendees.append([
                "name": participant.name ?? NSNull(),
                "url": participant.url.absoluteString,
                "is_current_user": participant.isCurrentUser,
            ])
        }
        let startDate: Date? = event.startDate
        let endDate: Date? = event.endDate
        out.append([
            "external_id": event.calendarItemExternalIdentifier ?? NSNull(),
            "event_id": event.eventIdentifier ?? NSNull(),
            "title": event.title ?? "",
            "start_ms": Int64(((startDate ?? start).timeIntervalSince1970 * 1000.0).rounded()),
            "end_ms": Int64(((endDate ?? startDate ?? start).timeIntervalSince1970 * 1000.0).rounded()),
            "all_day": event.isAllDay,
            "url": event.url?.absoluteString ?? NSNull(),
            "location": event.location ?? NSNull(),
            "notes": event.notes ?? NSNull(),
            "attendees": attendees,
        ])
    }

    do {
        let data = try JSONSerialization.data(withJSONObject: out, options: [])
        guard let json = String(data: data, encoding: .utf8) else {
            setError(errorOut, "calendar events are not valid UTF-8")
            return nil
        }
        return strdup(json)
    } catch {
        setError(errorOut, "serializing calendar events failed: \(error)")
        return nil
    }
}

// MARK: - Capture: shared pieces

private let outputSampleRate: Double = 16_000

/// Forwards converted audio to the Rust callbacks. `close()` guarantees that,
/// once it returns, no callback is running and none will start.
private final class CaptureSink {
    private let audioCallback: guidy_mb_audio_cb
    private let eventCallback: guidy_mb_event_cb?
    private let context: UnsafeMutableRawPointer?
    private let lock = NSLock()
    private var closed = false

    init(audio: guidy_mb_audio_cb, event: guidy_mb_event_cb?, context: UnsafeMutableRawPointer?) {
        self.audioCallback = audio
        self.eventCallback = event
        self.context = context
    }

    func audio(_ stream: Int32, _ samples: UnsafePointer<Float>, _ count: Int) {
        guard count > 0 else { return }
        lock.lock()
        defer { lock.unlock() }
        if closed { return }
        audioCallback(context, stream, samples, Int32(count), Int32(outputSampleRate))
    }

    func event(_ stream: Int32, _ message: String) {
        lock.lock()
        defer { lock.unlock() }
        if closed { return }
        guard let callback = eventCallback else { return }
        message.withCString { callback(context, stream, $0) }
    }

    func close() {
        lock.lock()
        closed = true
        lock.unlock()
    }
}

/// Converts mono f32 at any rate to 16 kHz mono f32 with AVAudioConverter.
/// The converter keeps its filter state between calls, so chunk boundaries are
/// seamless.
private final class MonoResampler {
    let inputFormat: AVAudioFormat
    private let outputFormat: AVAudioFormat
    private let converter: AVAudioConverter?

    init?(inputRate: Double) {
        guard inputRate > 0,
              let input = AVAudioFormat(
                  commonFormat: .pcmFormatFloat32, sampleRate: inputRate, channels: 1,
                  interleaved: false),
              let output = AVAudioFormat(
                  commonFormat: .pcmFormatFloat32, sampleRate: outputSampleRate, channels: 1,
                  interleaved: false)
        else { return nil }
        inputFormat = input
        outputFormat = output
        if inputRate == outputSampleRate {
            converter = nil
        } else {
            guard let c = AVAudioConverter(from: input, to: output) else { return nil }
            c.sampleRateConverterQuality = AVAudioQuality.high.rawValue
            converter = c
        }
    }

    /// Scratch buffer in `inputFormat` with room for `frames` frames.
    func makeInputBuffer(frames: Int) -> AVAudioPCMBuffer? {
        guard let buffer = AVAudioPCMBuffer(
            pcmFormat: inputFormat, frameCapacity: AVAudioFrameCount(max(frames, 1)))
        else { return nil }
        buffer.frameLength = AVAudioFrameCount(frames)
        return buffer
    }

    func process(_ input: AVAudioPCMBuffer, emit: (UnsafePointer<Float>, Int) -> Void) {
        guard input.frameLength > 0, let inData = input.floatChannelData else { return }
        guard let converter = converter else {
            emit(UnsafePointer(inData[0]), Int(input.frameLength))
            return
        }
        let ratio = outputSampleRate / inputFormat.sampleRate
        let capacity = AVAudioFrameCount(Double(input.frameLength) * ratio + 64)
        guard let output = AVAudioPCMBuffer(pcmFormat: outputFormat, frameCapacity: capacity)
        else { return }
        var fed = false
        var error: NSError?
        let status = converter.convert(to: output, error: &error) { _, inputStatus in
            if fed {
                inputStatus.pointee = .noDataNow
                return nil
            }
            fed = true
            inputStatus.pointee = .haveData
            return input
        }
        if status == .error { return }
        if output.frameLength > 0, let outData = output.floatChannelData {
            emit(UnsafePointer(outData[0]), Int(output.frameLength))
        }
    }
}

// MARK: - Capture: microphone (AVAudioEngine + Voice Processing I/O)

// Which microphone, and whether echo cancellation is worth it.
//
// Opening a Bluetooth headset's microphone switches it from its music
// profile (A2DP) to the hands-free one: about a second of silence for
// everything playing, narrowband mono audio until the mic is released (and
// a second gap then), and extra latency that puts video out of sync.
// Voice Processing I/O also takes over the output device, which interrupts
// what plays as it starts and stops. Echo cancellation is only needed when
// the meeting plays through speakers, so with headphones the plain built-in
// microphone is recorded and the output is left alone.

private func audioProperty<T>(
    _ id: AudioObjectID, _ selector: AudioObjectPropertySelector,
    _ scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal, _ initial: T
) -> T? {
    var address = AudioObjectPropertyAddress(
        mSelector: selector, mScope: scope, mElement: kAudioObjectPropertyElementMain)
    var value = initial
    var size = UInt32(MemoryLayout<T>.size)
    let status = AudioObjectGetPropertyData(id, &address, 0, nil, &size, &value)
    return status == noErr ? value : nil
}

private func defaultDevice(input: Bool) -> AudioObjectID? {
    let selector = input
        ? kAudioHardwarePropertyDefaultInputDevice : kAudioHardwarePropertyDefaultOutputDevice
    let id: AudioObjectID? = audioProperty(
        AudioObjectID(kAudioObjectSystemObject), selector, kAudioObjectPropertyScopeGlobal,
        AudioObjectID(kAudioObjectUnknown))
    return id.flatMap { $0 == kAudioObjectUnknown ? nil : $0 }
}

private func transportType(_ id: AudioObjectID) -> UInt32 {
    audioProperty(id, kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal, UInt32(0))
        ?? 0
}

private func isBluetooth(_ id: AudioObjectID) -> Bool {
    let t = transportType(id)
    return t == kAudioDeviceTransportTypeBluetooth || t == kAudioDeviceTransportTypeBluetoothLE
}

private func inputChannelCount(_ id: AudioObjectID) -> Int {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyStreamConfiguration, mScope: kAudioObjectPropertyScopeInput,
        mElement: kAudioObjectPropertyElementMain)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(id, &address, 0, nil, &size) == noErr, size > 0 else {
        return 0
    }
    let raw = UnsafeMutableRawPointer.allocate(byteCount: Int(size), alignment: 16)
    defer { raw.deallocate() }
    guard AudioObjectGetPropertyData(id, &address, 0, nil, &size, raw) == noErr else { return 0 }
    let list = UnsafeMutableAudioBufferListPointer(raw.assumingMemoryBound(to: AudioBufferList.self))
    return list.reduce(0) { $0 + Int($1.mNumberChannels) }
}

/// The Mac's own microphone, if it has one (a closed laptop's isn't listed).
private func builtInMicrophone() -> AudioObjectID? {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDevices, mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    var size: UInt32 = 0
    let system = AudioObjectID(kAudioObjectSystemObject)
    guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr else {
        return nil
    }
    var ids = [AudioObjectID](repeating: 0, count: Int(size) / MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(system, &address, 0, nil, &size, &ids) == noErr else {
        return nil
    }
    return ids.first {
        transportType($0) == kAudioDeviceTransportTypeBuiltIn && inputChannelCount($0) > 0
    }
}

/// Whether the default output is headphones: Bluetooth (AirPods and the
/// like), or the headphone jack. Speakers (and anything unknown) aren't.
private func outputIsHeadphones() -> Bool {
    guard let output = defaultDevice(input: false) else { return false }
    if isBluetooth(output) { return true }
    if transportType(output) == kAudioDeviceTransportTypeBuiltIn {
        // 'hdpn': the built-in output's headphone data source.
        let source: UInt32? = audioProperty(
            output, kAudioDevicePropertyDataSource, kAudioObjectPropertyScopeOutput, UInt32(0))
        return source == 0x6864_706E
    }
    return false
}

/// How to record the microphone right now.
private struct MicPlan {
    /// Echo cancellation (Voice Processing I/O, default devices only).
    let voiceProcessing: Bool
    /// The device to record, when not the default input.
    let device: AudioObjectID?
    let note: String?

    static func current() -> MicPlan {
        guard outputIsHeadphones() else {
            return MicPlan(voiceProcessing: true, device: nil, note: nil)
        }
        if let input = defaultDevice(input: true), isBluetooth(input),
           let builtIn = builtInMicrophone()
        {
            return MicPlan(
                voiceProcessing: false, device: builtIn,
                note: "headphones: recording the built-in microphone, so the Bluetooth headset keeps its high-quality audio")
        }
        return MicPlan(
            voiceProcessing: false, device: nil,
            note: "headphones: recording the microphone without echo cancellation")
    }
}

private final class MicCapture {
    /// Replaced (see `replaceEngine`) on `controlQueue`, or in `start`
    /// before anything else uses it.
    private var engine = AVAudioEngine()
    private let sink: CaptureSink
    /// Serializes restarts and stop.
    private let controlQueue = DispatchQueue(label: "dev.guidy.meetings.mic.control")
    /// Runs the silence timer.
    private let tickQueue = DispatchQueue(label: "dev.guidy.meetings.mic.tick", qos: .userInitiated)
    private let stateLock = NSLock()
    private var resampler: MonoResampler?
    private var observer: NSObjectProtocol?
    private var stopped = false
    /// Restart bookkeeping, on `controlQueue`: device changes arrive in
    /// bursts, so each one bumps the generation and stale retries drop out.
    private var restartGeneration = 0
    private var voiceProcessing = false
    /// The default input and output when echo cancellation last failed to
    /// start. On the same devices it isn't tried again this recording: each
    /// failed attempt reconfigures the hardware, which posts another
    /// configuration change, whose restart tried again (the mic flickered
    /// on and off for the whole meeting and recorded little but silence).
    private var voiceProcessingFailedOn: (input: AudioObjectID?, output: AudioObjectID?)?
    /// The input device recorded instead of the default one (headphones:
    /// the built-in mic). `nil`: the default input.
    private var device: AudioObjectID?
    /// Puts the mic on the session timeline (shared origin with the system
    /// stream). Guarded by `alignLock`: the tap block and the silence timer
    /// both use it. The lock is held only while converting/forwarding one
    /// buffer (the forward is a non-blocking channel send on the Rust side),
    /// never across a restart, so neither side waits for long. Lock order:
    /// `alignLock`, then the sink's lock.
    private let aligner: TimelineAligner
    private let alignLock = NSLock()
    private var silenceTimer: DispatchSourceTimer?

    init(sink: CaptureSink, originNanos: UInt64) {
        self.sink = sink
        self.aligner = TimelineAligner(startNanos: originNanos)
    }

    func start() throws {
        let plan = MicPlan.current()
        device = plan.device
        if let note = plan.note {
            sink.event(GUIDY_MB_STREAM_MIC, note)
        }
        if !plan.voiceProcessing {
            try installTapAndStart()
        } else {
            do {
                try startVoiceProcessing()
                voiceProcessing = true
            } catch {
                // Echo cancellation is preferred, not required: record the
                // plain mic rather than nothing, and say so. The failed
                // attempt already replaced the engine.
                voiceProcessingFailedOn = currentDevices()
                try installTapAndStart()
                sink.event(
                    GUIDY_MB_STREAM_MIC,
                    "echo cancellation unavailable, recording the plain microphone: \(error)")
            }
        }

        observeConfigurationChanges()
        startSilenceTimer()
    }

    private func currentDevices() -> (input: AudioObjectID?, output: AudioObjectID?) {
        (defaultDevice(input: true), defaultDevice(input: false))
    }

    /// Echo cancellation already failed on the devices in use now.
    private func voiceProcessingFailedHere() -> Bool {
        guard let failed = voiceProcessingFailedOn else { return false }
        let now = currentDevices()
        return failed.input == now.input && failed.output == now.output
    }

    /// Restart on device changes of the current engine.
    private func observeConfigurationChanges() {
        if let observer = observer {
            NotificationCenter.default.removeObserver(observer)
        }
        observer = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil
        ) { [weak self] _ in
            guard let self = self else { return }
            self.controlQueue.async {
                self.restartGeneration += 1
                self.restart(generation: self.restartGeneration, attempt: 0)
            }
        }
    }

    /// Swap in a fresh engine for the plain microphone. Voice processing
    /// (even a failed attempt) leaves the engine's output side configured:
    /// the main mixer connected to the output node, the VPIO output unit
    /// half set up. Starting that engine again then fails on the output's
    /// format (-10875, `IsFormatSampleRateAndChannelCountValid(outputHWFormat)`)
    /// although nothing plays. The plain path never touches the output, so a
    /// new engine has none to fail on.
    private func replaceEngine() {
        stopEngine()
        disableVoiceProcessing()
        engine = AVAudioEngine()
        if observer != nil {
            observeConfigurationChanges()
        }
    }

    /// While the mic delivers nothing (a VPIO restart in progress, a restart
    /// that failed), keep the track on the wall clock with silence.
    private func startSilenceTimer() {
        let timer = DispatchSource.makeTimerSource(queue: tickQueue)
        timer.schedule(deadline: .now() + .milliseconds(100), repeating: .milliseconds(100))
        timer.setEventHandler { [weak self] in
            guard let self = self else { return }
            let sink = self.sink
            self.alignLock.lock()
            self.aligner.tick(nowNanos: nowNanos()) { samples, count in
                sink.audio(GUIDY_MB_STREAM_MIC, samples, count)
            }
            self.alignLock.unlock()
        }
        timer.resume()
        silenceTimer = timer
    }

    private func startVoiceProcessing() throws {
        let input = engine.inputNode
        var enableError: Error?
        try objcTry("enabling voice processing") {
            do {
                try input.setVoiceProcessingEnabled(true)
            } catch {
                enableError = error
            }
        }
        if let error = enableError {
            replaceEngine()
            throw BridgeError("enabling voice processing (echo cancellation) failed: \(error)")
        }
        if #available(macOS 14.0, *) {
            // Keep other apps (the meeting itself) as loud as possible.
            input.voiceProcessingOtherAudioDuckingConfiguration =
                AVAudioVoiceProcessingOtherAudioDuckingConfiguration(
                    enableAdvancedDucking: false, duckingLevel: .min)
        }
        // VPIO runs input and output as one unit; give the engine an output
        // path (silent) so it renders.
        engine.mainMixerNode.outputVolume = 0

        do {
            try installTapAndStart()
        } catch {
            // `voiceProcessing` (set only on success) stays false, so the
            // next plain start must get an engine without voice processing.
            replaceEngine()
            throw error
        }
    }

    private func installTapAndStart() throws {
        let input = engine.inputNode
        // Without echo cancellation the device is chosen here (an earlier
        // start may have chosen another one on this unit).
        if !voiceProcessing, var id = device ?? defaultDevice(input: true) {
            // Before the format is read: it is the chosen device's.
            guard let unit = input.audioUnit else {
                throw BridgeError("the microphone has no audio unit to choose a device on")
            }
            let status = AudioUnitSetProperty(
                unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, &id,
                UInt32(MemoryLayout<AudioObjectID>.size))
            if status != noErr {
                // Recording the default input is still better than nothing.
                device = nil
                sink.event(
                    GUIDY_MB_STREAM_MIC,
                    "could not choose the built-in microphone (OSStatus \(status)), recording the default input")
            }
        }
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            throw BridgeError("no microphone input is available")
        }
        guard let resampler = MonoResampler(inputRate: format.sampleRate) else {
            throw BridgeError("cannot convert microphone audio at \(format.sampleRate) Hz")
        }
        stateLock.lock()
        self.resampler = resampler
        stateLock.unlock()

        // `format: nil` taps in the bus's current format. Passing a format
        // read a moment earlier raises an exception when the hardware changed
        // in between (seen after device switches); `handle` adapts instead.
        try objcTry("installing the microphone tap") {
            input.installTap(onBus: 0, bufferSize: 4096, format: nil) { [weak self] buffer, when in
                self?.handle(buffer, when)
            }
        }
        var startError: Error?
        do {
            try objcTry("starting the microphone") {
                self.engine.prepare()
                do {
                    try self.engine.start()
                } catch {
                    startError = error
                }
            }
        } catch {
            startError = error
        }
        if let error = startError {
            removeTap()
            throw BridgeError("starting the microphone failed: \(error)")
        }
    }

    /// Stop the engine and remove the tap; never throws (there is nothing to
    /// recover if either was already gone).
    private func stopEngine() {
        try? objcTry("stopping the microphone") { self.engine.stop() }
        removeTap()
    }

    private func removeTap() {
        try? objcTry("removing the microphone tap") { self.engine.inputNode.removeTap(onBus: 0) }
    }

    private func disableVoiceProcessing() {
        try? objcTry("disabling voice processing") {
            try? self.engine.inputNode.setVoiceProcessingEnabled(false)
        }
    }

    private func handle(_ buffer: AVAudioPCMBuffer, _ when: AVAudioTime) {
        stateLock.lock()
        let resampler = self.resampler
        stateLock.unlock()
        guard var resampler = resampler else { return }
        if buffer.format.sampleRate != resampler.inputFormat.sampleRate {
            // The device changed under a running tap.
            guard let rebuilt = MonoResampler(inputRate: buffer.format.sampleRate) else { return }
            stateLock.lock()
            self.resampler = rebuilt
            stateLock.unlock()
            resampler = rebuilt
        }
        let frames = Int(buffer.frameLength)
        guard frames > 0, let source = buffer.floatChannelData,
              let mono = resampler.makeInputBuffer(frames: frames),
              let destination = mono.floatChannelData?[0]
        else { return }
        // With voice processing the first channel carries the processed voice;
        // extra channels (some Macs report several) are not mixed in.
        if buffer.format.isInterleaved {
            let stride = max(buffer.stride, 1)
            let src = source[0]
            for i in 0..<frames {
                destination[i] = src[i * stride]
            }
        } else {
            destination.update(from: source[0], count: frames)
        }
        let capturedAt = captureNanos(
            hostTime: when.isHostTimeValid ? when.hostTime : nil,
            frames: frames, sampleRate: buffer.format.sampleRate)
        let sink = self.sink
        alignLock.lock()
        defer { alignLock.unlock() }
        resampler.process(mono) { samples, count in
            aligner.audio(samples, count, atNanos: capturedAt) { s, n in
                sink.audio(GUIDY_MB_STREAM_MIC, s, n)
            }
        }
    }

    /// The input or output device changed: rebuild the tap with the new format.
    /// The time the restart takes becomes silence on the track (the aligner
    /// pads up to the first new buffer's timestamp). Devices often settle a
    /// moment after the notification (Bluetooth profile switches, docks), so a
    /// failed restart is retried with backoff for as long as the recording
    /// runs; after a few failures, echo cancellation is given up for the
    /// plain microphone. Runs on `controlQueue`.
    private func restart(generation: Int, attempt: Int) {
        if stopped || generation != restartGeneration { return }
        stopEngine()
        // Headphones may have been put on or taken off: plan again.
        let plan = MicPlan.current()
        let wantVoiceProcessing =
            plan.voiceProcessing && attempt < 3 && !voiceProcessingFailedHere()
        if voiceProcessing && plan.voiceProcessing && attempt == 3 {
            sink.event(
                GUIDY_MB_STREAM_MIC,
                "echo cancellation unavailable after a device change, recording the plain microphone")
        }
        if voiceProcessing && !wantVoiceProcessing {
            replaceEngine()
            voiceProcessing = false
        }
        device = wantVoiceProcessing ? nil : plan.device
        do {
            if wantVoiceProcessing && !voiceProcessing {
                do {
                    try startVoiceProcessing()
                    voiceProcessing = true
                } catch {
                    // As at the start: the plain mic now, no retry of echo
                    // cancellation (and no meeting error) for this failure.
                    voiceProcessingFailedOn = currentDevices()
                    sink.event(
                        GUIDY_MB_STREAM_MIC,
                        "echo cancellation unavailable after a device change, recording the plain microphone: \(error)")
                    try installTapAndStart()
                }
            } else {
                try installTapAndStart()
            }
            sink.event(GUIDY_MB_STREAM_MIC, "microphone restarted after an audio device change")
        } catch {
            if attempt == 0 {
                // Kept as the meeting's error even if a retry succeeds later.
                sink.event(
                    GUIDY_MB_STREAM_MIC,
                    "error: \(error) (retrying while the recording runs)")
            }
            let delays = [0.25, 0.5, 1.0, 2.0]
            let delay = delays[min(attempt, delays.count - 1)]
            controlQueue.asyncAfter(deadline: .now() + delay) { [weak self] in
                self?.restart(generation: generation, attempt: attempt + 1)
            }
        }
    }

    func stop() {
        silenceTimer?.cancel()
        silenceTimer = nil
        controlQueue.sync {
            stopped = true
            // On `controlQueue`: a restart may be replacing the engine (and
            // its observer).
            if let observer = observer {
                NotificationCenter.default.removeObserver(observer)
                self.observer = nil
            }
            stopEngine()
        }
        // Wait out a tick already running.
        tickQueue.sync {}
        // Leave the shared input node in its normal mode for dictation.
        disableVoiceProcessing()
    }
}

// MARK: - Capture: system audio (Core Audio process tap, macOS 14.2+)

private func ownProcessAudioObject() -> AudioObjectID? {
    var pid = getpid()
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    var object = AudioObjectID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioObjectID>.size)
    let status = AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject), &address,
        UInt32(MemoryLayout<pid_t>.size), &pid, &size, &object)
    guard status == noErr, object != kAudioObjectUnknown else { return nil }
    return object
}

private func hostNanos(_ hostTime: UInt64) -> UInt64 {
    return AudioConvertHostTimeToNanos(hostTime)
}

private func nowNanos() -> UInt64 {
    return AudioConvertHostTimeToNanos(AudioGetCurrentHostTime())
}

/// Puts one capture stream on the session's wall-clock timeline, so that
/// sample `n` of the stream was captured (within `tolerance`) `n / 16 kHz`
/// seconds after `startNanos`, the moment `guidy_mb_capture_start` was called.
/// The mic and the system stream each own one aligner, built from the same
/// `startNanos`, so the two tracks share one time origin.
///
/// Rules, applied to every chunk of converted (16 kHz) audio:
/// - The chunk's first sample belongs at `target = (t - start) * 16 kHz`.
/// - `target` more than `tolerance` ahead of what was emitted: a gap (the
///   stream started late, a process tap delivered nothing while no app played
///   audio, the mic's VPIO unit restarted after a device or configuration
///   change, ...). Silence is padded up to `target`, then the chunk follows.
/// - `target` more than `tolerance` behind: the chunk overlaps audio (or
///   padded silence) already emitted. The overlapping head is dropped and only
///   the rest is emitted; a chunk that overlaps entirely is dropped.
/// - Within `tolerance`: emitted as is. Timestamp jitter and resampler latency
///   are not "corrected", which would click; drift is corrected in one step
///   once it exceeds `tolerance`.
/// - [`tick`] pads silence while a stream delivers nothing for a while, so a
///   silent or dead stream keeps pace with the wall clock.
///
/// Pure: every time is passed in, nothing reads a clock, nothing blocks, so it
/// is unit tested from Rust through the `guidy_mb_test_aligner_*` hooks.
/// Not thread safe: each owner serializes its calls (see the call sites).
private final class TimelineAligner {
    private let startNanos: UInt64
    private(set) var emitted: Int64 = 0
    /// Wall-clock time just past the last real audio sample.
    private var lastAudioEndNanos: UInt64
    private let tolerance: Int64
    private let zeros = [Float](repeating: 0, count: 1_600)

    /// Default tolerance: 50 ms.
    static let defaultTolerance: Int64 = 800
    /// `tick` pads only after this much time without audio...
    static let tickIdleNanos: UInt64 = 300_000_000
    /// ...and leaves this much room for audio still in flight.
    static let tickLagNanos: UInt64 = 100_000_000

    init(startNanos: UInt64, tolerance: Int64 = TimelineAligner.defaultTolerance) {
        self.startNanos = startNanos
        self.lastAudioEndNanos = startNanos
        self.tolerance = max(tolerance, 0)
    }

    private func position(atNanos t: UInt64) -> Int64 {
        guard t > startNanos else { return 0 }
        return Int64(Double(t - startNanos) / 1e9 * outputSampleRate)
    }

    private func padSilence(to target: Int64, _ emit: (UnsafePointer<Float>, Int) -> Void) {
        zeros.withUnsafeBufferPointer { zero in
            while emitted < target {
                let count = Int(min(Int64(zero.count), target - emitted))
                emit(zero.baseAddress!, count)
                emitted += Int64(count)
            }
        }
    }

    /// 16 kHz audio whose first sample was captured at `nanos` (callers clamp
    /// `nanos` to "now", so a bogus timestamp can't pad into the future).
    func audio(
        _ samples: UnsafePointer<Float>, _ count: Int, atNanos nanos: UInt64,
        _ emit: (UnsafePointer<Float>, Int) -> Void
    ) {
        guard count > 0 else { return }
        let durationNanos = UInt64(Double(count) / outputSampleRate * 1e9)
        lastAudioEndNanos = max(lastAudioEndNanos, nanos &+ durationNanos)
        let target = position(atNanos: nanos)
        var skip = 0
        if target > emitted + tolerance {
            padSilence(to: target, emit)
        } else if target < emitted - tolerance {
            // Audio or silence was emitted past this point already: drop the
            // overlapping head.
            skip = Int(min(Int64(count), emitted - target))
        }
        if skip < count {
            emit(samples + skip, count - skip)
            emitted += Int64(count - skip)
        }
    }

    /// Called periodically with the current time: fills a silence that has
    /// lasted a while (no audio for `tickIdleNanos`), up to `tickLagNanos`
    /// before now.
    func tick(nowNanos now: UInt64, _ emit: (UnsafePointer<Float>, Int) -> Void) {
        guard now > lastAudioEndNanos &+ TimelineAligner.tickIdleNanos else { return }
        padSilence(to: position(atNanos: now - TimelineAligner.tickLagNanos), emit)
    }
}

/// Capture time of a buffer's first sample: its host time when valid, else an
/// estimate from "now" minus the buffer's duration. Never later than now.
private func captureNanos(hostTime: UInt64?, frames: Int, sampleRate: Double) -> UInt64 {
    let now = nowNanos()
    if let hostTime = hostTime {
        return min(hostNanos(hostTime), now)
    }
    let duration = sampleRate > 0 ? UInt64(Double(frames) / sampleRate * 1e9) : 0
    return now > duration ? now - duration : 0
}

// MARK: - Aligner test hooks (called only by the Rust unit tests)

/// Collects what an aligner emits into a caller buffer for the test hooks.
private func collect(
    into out: UnsafeMutablePointer<Float>?, capacity: Int64,
    _ body: ((UnsafePointer<Float>, Int) -> Void) -> Void
) -> Int64 {
    var written: Int64 = 0
    body { samples, count in
        if let out = out {
            let room = max(0, capacity - written)
            let n = Int(min(Int64(count), room))
            if n > 0 { (out + Int(written)).update(from: samples, count: n) }
        }
        written += Int64(count)
    }
    return written
}

@_cdecl("guidy_mb_test_aligner_new")
public func guidyMbTestAlignerNew(_ startNanos: UInt64, _ toleranceSamples: Int64)
    -> UnsafeMutableRawPointer
{
    let aligner = TimelineAligner(startNanos: startNanos, tolerance: toleranceSamples)
    return Unmanaged.passRetained(aligner).toOpaque()
}

@_cdecl("guidy_mb_test_aligner_audio")
public func guidyMbTestAlignerAudio(
    _ handle: UnsafeMutableRawPointer?, _ samples: UnsafePointer<Float>?, _ count: Int32,
    _ atNanos: UInt64, _ out: UnsafeMutablePointer<Float>?, _ capacity: Int64
) -> Int64 {
    guard let handle = handle, let samples = samples, count > 0 else { return 0 }
    let aligner = Unmanaged<TimelineAligner>.fromOpaque(handle).takeUnretainedValue()
    return collect(into: out, capacity: capacity) { emit in
        aligner.audio(samples, Int(count), atNanos: atNanos, emit)
    }
}

@_cdecl("guidy_mb_test_aligner_tick")
public func guidyMbTestAlignerTick(
    _ handle: UnsafeMutableRawPointer?, _ nowNanos: UInt64,
    _ out: UnsafeMutablePointer<Float>?, _ capacity: Int64
) -> Int64 {
    guard let handle = handle else { return 0 }
    let aligner = Unmanaged<TimelineAligner>.fromOpaque(handle).takeUnretainedValue()
    return collect(into: out, capacity: capacity) { emit in
        aligner.tick(nowNanos: nowNanos, emit)
    }
}

@_cdecl("guidy_mb_test_aligner_free")
public func guidyMbTestAlignerFree(_ handle: UnsafeMutableRawPointer?) {
    guard let handle = handle else { return }
    Unmanaged<TimelineAligner>.fromOpaque(handle).release()
}

@available(macOS 14.2, *)
private final class SystemTapCapture {
    /// `nil` for the permission probe: audio is discarded.
    private let sink: CaptureSink?
    /// The IOProc block runs here, off the HAL's real-time thread.
    private let ioQueue = DispatchQueue(label: "dev.guidy.meetings.tap.io", qos: .userInitiated)
    /// Serializes rebuilds (device changes) and stop.
    private let controlQueue = DispatchQueue(label: "dev.guidy.meetings.tap.control")

    private var tapID = AudioObjectID(kAudioObjectUnknown)
    private var tapUID = ""
    private var aggregateID = AudioObjectID(kAudioObjectUnknown)
    private var ioProcID: AudioDeviceIOProcID?
    private var listener: AudioObjectPropertyListenerBlock?
    private var stopped = false

    // Touched only on ioQueue (and while the IOProc is stopped).
    private var resampler: MonoResampler?
    private var scratch: AVAudioPCMBuffer?
    private var aligner: TimelineAligner?
    private var silenceTimer: DispatchSourceTimer?

    private(set) var framesReceived = 0
    private(set) var peak: Float = 0
    /// The session's time origin (shared with the mic stream).
    private let originNanos: UInt64

    init(sink: CaptureSink?, originNanos: UInt64) {
        self.sink = sink
        self.originNanos = originNanos
    }

    func start() throws {
        // Exclude Guidy's own sounds. The process only has an audio object once
        // it has touched Core Audio, which is why the mic starts first.
        let own = ownProcessAudioObject()
        // The raw Objective-C initializer (no Swift overlay symbol needed).
        let excluded: [NSNumber] = own.map { [NSNumber(value: $0)] } ?? []
        let description = CATapDescription(__stereoGlobalTapButExcludeProcesses: excluded)
        description.name = "Guidy meeting notes"
        description.isPrivate = true
        description.muteBehavior = .unmuted

        var tap = AudioObjectID(kAudioObjectUnknown)
        let status = AudioHardwareCreateProcessTap(description, &tap)
        guard status == noErr, tap != kAudioObjectUnknown else {
            throw BridgeError("creating the system audio tap failed (OSStatus \(status))")
        }
        tapID = tap
        tapUID = description.uuid.uuidString

        do {
            try buildDevice()
        } catch {
            teardownDevice()
            AudioHardwareDestroyProcessTap(tapID)
            tapID = AudioObjectID(kAudioObjectUnknown)
            throw error
        }
        installDefaultOutputListener()
        startSilenceTimer()
    }

    private func startSilenceTimer() {
        guard let sink = sink else { return }
        // The aligner lives on ioQueue: the IOProc block and this timer both
        // run there, so its calls are serialized without a lock.
        let aligner = TimelineAligner(startNanos: originNanos)
        ioQueue.sync { self.aligner = aligner }
        let timer = DispatchSource.makeTimerSource(queue: ioQueue)
        timer.schedule(deadline: .now() + .milliseconds(100), repeating: .milliseconds(100))
        timer.setEventHandler {
            aligner.tick(nowNanos: nowNanos()) { samples, count in
                sink.audio(GUIDY_MB_STREAM_SYSTEM, samples, count)
            }
        }
        timer.resume()
        silenceTimer = timer
    }

    private func readTapFormat() throws -> AudioStreamBasicDescription {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioTapPropertyFormat,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        var format = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        let status = AudioObjectGetPropertyData(tapID, &address, 0, nil, &size, &format)
        guard status == noErr else {
            throw BridgeError("reading the system audio tap format failed (OSStatus \(status))")
        }
        let isFloat = format.mFormatFlags & kAudioFormatFlagIsFloat != 0
        guard format.mFormatID == kAudioFormatLinearPCM, isFloat, format.mBitsPerChannel == 32,
              format.mSampleRate > 0
        else {
            throw BridgeError(
                "unsupported system audio format (id \(format.mFormatID), flags \(format.mFormatFlags), \(format.mBitsPerChannel) bits)"
            )
        }
        return format
    }

    /// Creates the private aggregate device around the tap and starts its
    /// IOProc. The aggregate holds only the tap (no sub-devices), so it does
    /// not depend on which output device is current, and it never mixes in a
    /// headset's microphone streams.
    private func buildDevice() throws {
        let format = try readTapFormat()
        guard let resampler = MonoResampler(inputRate: format.mSampleRate) else {
            throw BridgeError("cannot convert system audio at \(format.mSampleRate) Hz")
        }
        ioQueue.sync {
            self.resampler = resampler
            self.scratch = nil
        }

        let description: [String: Any] = [
            kAudioAggregateDeviceNameKey: "Guidy Meeting Tap",
            kAudioAggregateDeviceUIDKey: "dev.guidy.meetings.tap." + UUID().uuidString,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [] as [Any],
            kAudioAggregateDeviceTapListKey: [
                [
                    kAudioSubTapUIDKey: tapUID,
                    kAudioSubTapDriftCompensationKey: true,
                ]
            ],
        ]
        var aggregate = AudioObjectID(kAudioObjectUnknown)
        var status = AudioHardwareCreateAggregateDevice(description as CFDictionary, &aggregate)
        guard status == noErr, aggregate != kAudioObjectUnknown else {
            throw BridgeError("creating the system audio device failed (OSStatus \(status))")
        }
        aggregateID = aggregate

        var procID: AudioDeviceIOProcID?
        status = AudioDeviceCreateIOProcIDWithBlock(&procID, aggregate, ioQueue) {
            [weak self] _, inputData, inputTime, _, _ in
            self?.handle(inputData, inputTime)
        }
        guard status == noErr, let proc = procID else {
            throw BridgeError("creating the system audio callback failed (OSStatus \(status))")
        }
        ioProcID = proc

        status = AudioDeviceStart(aggregate, proc)
        guard status == noErr else {
            throw BridgeError("starting system audio capture failed (OSStatus \(status))")
        }
    }

    private func teardownDevice() {
        if let proc = ioProcID {
            AudioDeviceStop(aggregateID, proc)
            AudioDeviceDestroyIOProcID(aggregateID, proc)
            ioProcID = nil
        }
        // Drain any IOProc block already queued.
        ioQueue.sync {}
        if aggregateID != kAudioObjectUnknown {
            AudioHardwareDestroyAggregateDevice(aggregateID)
            aggregateID = AudioObjectID(kAudioObjectUnknown)
        }
    }

    private func defaultOutputAddress() -> AudioObjectPropertyAddress {
        return AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
    }

    /// When the default output device changes the tap's format (sample rate)
    /// may change too, so the aggregate and its IOProc are rebuilt.
    private func installDefaultOutputListener() {
        let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
            self?.rebuild()
        }
        var address = defaultOutputAddress()
        let status = AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject), &address, controlQueue, block)
        if status == noErr {
            listener = block
        }
    }

    /// Runs on controlQueue.
    private func rebuild() {
        if stopped { return }
        teardownDevice()
        do {
            try buildDevice()
            sink?.event(GUIDY_MB_STREAM_SYSTEM, "system audio restarted after an output device change")
        } catch {
            teardownDevice()
            sink?.event(GUIDY_MB_STREAM_SYSTEM, "error: \(error)")
        }
    }

    /// Runs on ioQueue.
    private func handle(
        _ inputData: UnsafePointer<AudioBufferList>, _ inputTime: UnsafePointer<AudioTimeStamp>
    ) {
        let buffers = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inputData))
        guard let resampler = resampler else { return }

        var frames = 0
        for buffer in buffers where buffer.mNumberChannels > 0 && buffer.mData != nil {
            let count = Int(buffer.mDataByteSize) / (4 * Int(buffer.mNumberChannels))
            frames = frames == 0 ? count : min(frames, count)
        }
        guard frames > 0 else { return }

        if scratch == nil || Int(scratch!.frameCapacity) < frames {
            scratch = resampler.makeInputBuffer(frames: max(frames, 4096))
        }
        guard let mono = scratch, let destination = mono.floatChannelData?[0] else { return }
        mono.frameLength = AVAudioFrameCount(frames)
        destination.initialize(repeating: 0, count: frames)

        // Average every channel of every buffer (interleaved or not) to mono.
        var channels = 0
        for buffer in buffers {
            guard let data = buffer.mData, buffer.mNumberChannels > 0 else { continue }
            let stride = Int(buffer.mNumberChannels)
            let samples = data.assumingMemoryBound(to: Float.self)
            for i in 0..<frames {
                var sum: Float = 0
                let base = i * stride
                for c in 0..<stride {
                    sum += samples[base + c]
                }
                destination[i] += sum
            }
            channels += stride
        }
        guard channels > 0 else { return }
        let scale = 1 / Float(channels)
        var localPeak: Float = 0
        for i in 0..<frames {
            destination[i] *= scale
            localPeak = max(localPeak, abs(destination[i]))
        }
        framesReceived += frames
        peak = max(peak, localPeak)

        guard let sink = sink, let aligner = aligner else { return }
        let time = inputTime.pointee
        let capturedAt = captureNanos(
            hostTime: time.mFlags.contains(.hostTimeValid) ? time.mHostTime : nil,
            frames: frames, sampleRate: resampler.inputFormat.sampleRate)
        let emit: (UnsafePointer<Float>, Int) -> Void = { samples, count in
            sink.audio(GUIDY_MB_STREAM_SYSTEM, samples, count)
        }
        resampler.process(mono) { samples, count in
            aligner.audio(samples, count, atNanos: capturedAt, emit)
        }
    }

    func stop() {
        controlQueue.sync {
            stopped = true
        }
        if let block = listener {
            var address = defaultOutputAddress()
            AudioObjectRemovePropertyListenerBlock(
                AudioObjectID(kAudioObjectSystemObject), &address, controlQueue, block)
            listener = nil
        }
        silenceTimer?.cancel()
        silenceTimer = nil
        controlQueue.sync {
            teardownDevice()
            if tapID != kAudioObjectUnknown {
                AudioHardwareDestroyProcessTap(tapID)
                tapID = AudioObjectID(kAudioObjectUnknown)
            }
        }
    }
}

private final class CaptureSession {
    let sink: CaptureSink
    var mic: MicCapture?
    var system: AnyObject?

    init(sink: CaptureSink) {
        self.sink = sink
    }

    func stop() {
        // First cut the callbacks so Rust can free its context right after.
        sink.close()
        if #available(macOS 14.2, *) {
            (system as? SystemTapCapture)?.stop()
        }
        system = nil
        mic?.stop()
        mic = nil
    }
}

@_cdecl("guidy_mb_capture_start")
public func guidyMbCaptureStart(
    _ wantMic: Int32,
    _ wantSystem: Int32,
    _ audioCallback: guidy_mb_audio_cb?,
    _ eventCallback: guidy_mb_event_cb?,
    _ context: UnsafeMutableRawPointer?,
    _ errorOut: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    guard let audioCallback = audioCallback else {
        setError(errorOut, "no audio callback")
        return nil
    }
    guard wantMic != 0 || wantSystem != 0 else {
        setError(errorOut, "no capture stream requested")
        return nil
    }
    let session = CaptureSession(
        sink: CaptureSink(audio: audioCallback, event: eventCallback, context: context))
    // The one time origin of both tracks: sample n of either stream was
    // captured n / 16 kHz seconds after this instant (see TimelineAligner).
    // Taken before either stream starts, so the VPIO start-up time becomes
    // leading silence on the mic track and the system stream does not start
    // its clock late.
    let originNanos = nowNanos()

    if wantMic != 0 {
        let mic = MicCapture(sink: session.sink, originNanos: originNanos)
        do {
            try mic.start()
        } catch {
            session.stop()
            mic.stop()
            setError(errorOut, "microphone: \(error)")
            return nil
        }
        session.mic = mic
    }

    if wantSystem != 0 {
        guard #available(macOS 14.2, *) else {
            session.stop()
            setError(errorOut, "system audio capture needs macOS 14.2 or newer")
            return nil
        }
        let system = SystemTapCapture(sink: session.sink, originNanos: originNanos)
        do {
            try system.start()
        } catch {
            session.stop()
            setError(errorOut, "system audio: \(error)")
            return nil
        }
        session.system = system
    }

    return Unmanaged.passRetained(session).toOpaque()
}

@_cdecl("guidy_mb_capture_stop")
public func guidyMbCaptureStop(_ handle: UnsafeMutableRawPointer?) {
    guard let handle = handle else { return }
    let session = Unmanaged<CaptureSession>.fromOpaque(handle).takeRetainedValue()
    session.stop()
}

// MARK: - System audio permission

// There is no public API to query or pre-request the "audio capture" TCC
// permission. The private TCC functions are looked up at runtime (best effort,
// the approach community ports of Apple's process-tap sample use); without
// them a short-lived tap triggers the prompt.
//
// Private API rules here:
// - They never run on the main thread: each call is made on a global queue.
//   The preflight is waited for at most `tccPreflightTimeout`; the request
//   (which shows a prompt and waits for the user) at most
//   `tccRequestTimeout`, after which the preflight is reported instead.
// - Any result we don't recognise is reported as UNKNOWN, never trusted.
private typealias TCCPreflightFn = @convention(c) (CFString, CFDictionary?) -> Int32
private typealias TCCRequestFn = @convention(c) (
    CFString, CFDictionary?, @escaping @convention(block) (Bool) -> Void
) -> Void

private let tccHandle: UnsafeMutableRawPointer? = dlopen(
    "/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC", RTLD_NOW)
private let audioCaptureService = "kTCCServiceAudioCapture" as CFString
private let tccPreflightTimeout: Double = 2
private let tccRequestTimeout: Double = 60
/// How long the fallback probe tap captures.
private let probeCaptureSeconds: Double = 1.5

private func tccPreflight() -> TCCPreflightFn? {
    guard let handle = tccHandle, let symbol = dlsym(handle, "TCCAccessPreflight") else {
        return nil
    }
    return unsafeBitCast(symbol, to: TCCPreflightFn.self)
}

private func tccRequest() -> TCCRequestFn? {
    guard let handle = tccHandle, let symbol = dlsym(handle, "TCCAccessRequest") else {
        return nil
    }
    return unsafeBitCast(symbol, to: TCCRequestFn.self)
}

/// Runs `work` on a global queue and waits up to `timeout` seconds for it.
/// `nil` on timeout (the work still finishes in the background; its result is
/// dropped).
private func offMainThread<T>(timeout: Double, _ work: @escaping () -> T) -> T? {
    let box = Box<T?>(nil)
    let semaphore = DispatchSemaphore(value: 0)
    DispatchQueue.global(qos: .userInitiated).async {
        box.value = work()
        semaphore.signal()
    }
    if semaphore.wait(timeout: .now() + timeout) == .timedOut {
        return nil
    }
    return box.value
}

/// TCCAccessPreflight result -> GUIDY_MB_AUTH_*. 0 granted, 1 denied, 2 "not
/// decided yet"; anything else is not something we know how to read.
private func mapPreflight(_ raw: Int32?) -> Int32 {
    switch raw {
    case .some(0): return GUIDY_MB_AUTH_GRANTED
    case .some(1): return GUIDY_MB_AUTH_DENIED
    case .some(2): return GUIDY_MB_AUTH_NOT_DETERMINED
    default: return GUIDY_MB_AUTH_UNKNOWN
    }
}

private func preflightStatus() -> Int32 {
    guard let preflight = tccPreflight() else { return GUIDY_MB_AUTH_UNKNOWN }
    return mapPreflight(
        offMainThread(timeout: tccPreflightTimeout) { preflight(audioCaptureService, nil) })
}

@_cdecl("guidy_mb_system_audio_status")
public func guidyMbSystemAudioStatus() -> Int32 {
    guard #available(macOS 14.2, *) else { return GUIDY_MB_AUTH_UNAVAILABLE }
    return preflightStatus()
}

@_cdecl("guidy_mb_request_system_audio_access")
public func guidyMbRequestSystemAudioAccess() -> Int32 {
    guard #available(macOS 14.2, *) else { return GUIDY_MB_AUTH_UNAVAILABLE }
    let current = preflightStatus()
    if current == GUIDY_MB_AUTH_GRANTED || current == GUIDY_MB_AUTH_DENIED {
        return current
    }
    if isBundled() && !hasUsageDescription("NSAudioCaptureUsageDescription") {
        NSLog("[guidy meetings] Info.plist lacks NSAudioCaptureUsageDescription")
        return GUIDY_MB_AUTH_DENIED
    }

    if let request = tccRequest() {
        if Thread.isMainThread {
            // Waiting for the user here would freeze the UI: show the prompt
            // and report what is known now. Callers should use a background
            // thread to get the answer.
            DispatchQueue.global(qos: .userInitiated).async {
                request(audioCaptureService, nil) { _ in }
            }
            NSLog("[guidy meetings] system audio access requested from the main thread; not waiting")
            return current
        }
        let answer: Bool?? = offMainThread(timeout: tccRequestTimeout) {
            let semaphore = DispatchSemaphore(value: 0)
            let granted = Box<Bool?>(nil)
            request(audioCaptureService, nil) { ok in
                granted.value = ok
                semaphore.signal()
            }
            if semaphore.wait(timeout: .now() + tccRequestTimeout) == .timedOut {
                return nil
            }
            return granted.value
        }
        switch answer {
        case .some(.some(true)): return GUIDY_MB_AUTH_GRANTED
        case .some(.some(false)): return GUIDY_MB_AUTH_DENIED
        default: return preflightStatus()
        }
    }

    // Fallback probe: a tap that runs for `probeCaptureSeconds` makes the
    // system ask. The tap's own result is preferred over the preflight: a tap
    // that can't start means capture won't work (DENIED), a tap that hears
    // non-silent audio means it does (GRANTED). Only a silent tap (no app was
    // playing, or the permission is missing and TCC fed zeros) falls back to
    // the preflight. Off the main thread: it sleeps.
    let probeOnce: () -> Int32 = {
        let probe = SystemTapCapture(sink: nil, originNanos: nowNanos())
        do {
            try probe.start()
        } catch {
            NSLog("[guidy meetings] system audio probe failed: %@", "\(error)")
            return GUIDY_MB_AUTH_DENIED
        }
        Thread.sleep(forTimeInterval: probeCaptureSeconds)
        // stop() drains the IO queue, so the counters are safe to read after.
        probe.stop()
        if probe.framesReceived > 0 && probe.peak > 0 {
            return GUIDY_MB_AUTH_GRANTED
        }
        return preflightStatus()
    }
    if Thread.isMainThread {
        return offMainThread(timeout: probeCaptureSeconds + tccPreflightTimeout + 5, probeOnce)
            ?? GUIDY_MB_AUTH_UNKNOWN
    }
    return probeOnce()
}

// MARK: - Notifications (UNUserNotificationCenter)

private let notificationLock = NSLock()
private var notificationCallback: guidy_mb_notification_cb?
private var notificationCategories: [String: UNNotificationCategory] = [:]

private final class NotificationDelegate: NSObject, UNUserNotificationCenterDelegate {
    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let notificationID = response.notification.request.identifier
        var actionID = response.actionIdentifier
        if actionID == UNNotificationDefaultActionIdentifier {
            actionID = "default"
        } else if actionID == UNNotificationDismissActionIdentifier {
            actionID = "dismiss"
        }
        notificationLock.lock()
        let callback = notificationCallback
        notificationLock.unlock()
        if let callback = callback {
            notificationID.withCString { n in
                actionID.withCString { a in callback(n, a) }
            }
        }
        completionHandler()
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        // Guidy is a menu-bar app and often counts as frontmost: show anyway.
        if #available(macOS 11.0, *) {
            completionHandler([.banner, .list, .sound])
        } else {
            completionHandler([.alert, .sound])
        }
    }
}

private let notificationDelegate = NotificationDelegate()

/// The notification center, or nil when the process has no bundle identifier
/// (UNUserNotificationCenter raises an exception in that case).
private func notificationCenter() -> UNUserNotificationCenter? {
    guard isBundled() else { return nil }
    let center = UNUserNotificationCenter.current()
    if center.delegate !== notificationDelegate {
        center.delegate = notificationDelegate
    }
    return center
}

@_cdecl("guidy_mb_notifications_available")
public func guidyMbNotificationsAvailable() -> Int32 {
    return isBundled() ? 1 : 0
}

@_cdecl("guidy_mb_notification_auth_status")
public func guidyMbNotificationAuthStatus() -> Int32 {
    guard let center = notificationCenter() else { return GUIDY_MB_AUTH_UNAVAILABLE }
    let semaphore = DispatchSemaphore(value: 0)
    let result = Box<Int32>(GUIDY_MB_AUTH_UNKNOWN)
    center.getNotificationSettings { settings in
        switch settings.authorizationStatus {
        case .notDetermined: result.value = GUIDY_MB_AUTH_NOT_DETERMINED
        case .denied: result.value = GUIDY_MB_AUTH_DENIED
        case .authorized, .provisional: result.value = GUIDY_MB_AUTH_GRANTED
        default: result.value = GUIDY_MB_AUTH_GRANTED
        }
        semaphore.signal()
    }
    if semaphore.wait(timeout: .now() + 5) == .timedOut {
        return GUIDY_MB_AUTH_UNKNOWN
    }
    return result.value
}

@_cdecl("guidy_mb_request_notification_access")
public func guidyMbRequestNotificationAccess() -> Int32 {
    guard let center = notificationCenter() else { return 0 }
    let semaphore = DispatchSemaphore(value: 0)
    let granted = Box(false)
    center.requestAuthorization(options: [.alert, .sound]) { ok, error in
        if let error = error {
            NSLog("[guidy meetings] notification authorization failed: %@", "\(error)")
        }
        granted.value = ok
        semaphore.signal()
    }
    semaphore.wait()
    return granted.value ? 1 : 0
}

/// Registers (once) a category holding the given actions and returns its id.
private func categoryIdentifier(
    for actions: [(String, String)], center: UNUserNotificationCenter
) -> String? {
    guard !actions.isEmpty else { return nil }
    let identifier = "dev.guidy.meetings." + actions.map { $0.0 }.joined(separator: "+")

    notificationLock.lock()
    let isNew = notificationCategories[identifier] == nil
    if isNew {
        let unActions = actions.map {
            UNNotificationAction(identifier: $0.0, title: $0.1, options: [])
        }
        notificationCategories[identifier] = UNNotificationCategory(
            identifier: identifier, actions: unActions, intentIdentifiers: [], options: [])
    }
    let all = Set(notificationCategories.values)
    notificationLock.unlock()

    if isNew {
        center.setNotificationCategories(all)
        // Registration is asynchronous; a round-trip makes sure the category
        // is known before the notification that uses it is added.
        let semaphore = DispatchSemaphore(value: 0)
        center.getNotificationCategories { _ in semaphore.signal() }
        _ = semaphore.wait(timeout: .now() + 2)
    }
    return identifier
}

@_cdecl("guidy_mb_post_notification")
public func guidyMbPostNotification(
    _ notificationID: UnsafePointer<CChar>?,
    _ title: UnsafePointer<CChar>?,
    _ body: UnsafePointer<CChar>?,
    _ actionsJSON: UnsafePointer<CChar>?
) -> Int32 {
    guard let center = notificationCenter(), let notificationID = notificationID else { return 0 }

    var actions: [(String, String)] = []
    if let actionsJSON = actionsJSON,
       let data = String(cString: actionsJSON).data(using: .utf8),
       let parsed = try? JSONSerialization.jsonObject(with: data) as? [[String]]
    {
        for pair in parsed where pair.count == 2 {
            actions.append((pair[0], pair[1]))
        }
    }

    let content = UNMutableNotificationContent()
    content.title = title.map { String(cString: $0) } ?? ""
    content.body = body.map { String(cString: $0) } ?? ""
    content.sound = .default
    if let category = categoryIdentifier(for: actions, center: center) {
        content.categoryIdentifier = category
    }

    let request = UNNotificationRequest(
        identifier: String(cString: notificationID), content: content, trigger: nil)
    center.add(request) { error in
        if let error = error {
            NSLog("[guidy meetings] posting a notification failed: %@", "\(error)")
        }
    }
    return 1
}

@_cdecl("guidy_mb_remove_notification")
public func guidyMbRemoveNotification(_ notificationID: UnsafePointer<CChar>?) {
    guard let center = notificationCenter(), let notificationID = notificationID else { return }
    let identifier = String(cString: notificationID)
    center.removePendingNotificationRequests(withIdentifiers: [identifier])
    center.removeDeliveredNotifications(withIdentifiers: [identifier])
}

@_cdecl("guidy_mb_set_notification_handler")
public func guidyMbSetNotificationHandler(_ callback: guidy_mb_notification_cb?) {
    notificationLock.lock()
    notificationCallback = callback
    notificationLock.unlock()
    // Install the delegate now so a click that launched/activated the app is
    // delivered.
    _ = notificationCenter()
}

// MARK: - Calls in other apps (who holds the microphone)

/// A JSON array of strings, or nil.
private func jsonStrings(_ values: [String]) -> UnsafeMutablePointer<CChar>? {
    guard let data = try? JSONSerialization.data(withJSONObject: values, options: []),
        let json = String(data: data, encoding: .utf8)
    else { return nil }
    return strdup(json)
}

@available(macOS 14.0, *)
private func processBundleID(_ id: AudioObjectID) -> String? {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioProcessPropertyBundleID, mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain)
    var value: Unmanaged<CFString>?
    var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    let status = withUnsafeMutablePointer(to: &value) {
        AudioObjectGetPropertyData(id, &address, 0, nil, &size, $0)
    }
    guard status == noErr, let cf = value?.takeRetainedValue() else { return nil }
    let bundle = cf as String
    return bundle.isEmpty ? nil : bundle
}

@_cdecl("guidy_mb_audio_input_bundle_ids")
public func guidyMbAudioInputBundleIds() -> UnsafeMutablePointer<CChar>? {
    guard #available(macOS 14.0, *) else { return jsonStrings([]) }
    let system = AudioObjectID(kAudioObjectSystemObject)
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyProcessObjectList,
        mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain)
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr, size > 0
    else { return jsonStrings([]) }
    var objects = [AudioObjectID](
        repeating: AudioObjectID(kAudioObjectUnknown),
        count: Int(size) / MemoryLayout<AudioObjectID>.size)
    guard AudioObjectGetPropertyData(system, &address, 0, nil, &size, &objects) == noErr else {
        return jsonStrings([])
    }
    let own = getpid()
    var out: [String] = []
    for object in objects.prefix(Int(size) / MemoryLayout<AudioObjectID>.size) {
        let running = audioProperty(
            object, kAudioProcessPropertyIsRunningInput, kAudioObjectPropertyScopeGlobal, UInt32(0))
        let pid = audioProperty(
            object, kAudioProcessPropertyPID, kAudioObjectPropertyScopeGlobal, pid_t(0))
        guard (running ?? 0) != 0, pid != own,
            let bundle = processBundleID(object), !out.contains(bundle)
        else { continue }
        out.append(bundle)
    }
    return jsonStrings(out)
}

@_cdecl("guidy_mb_window_titles")
public func guidyMbWindowTitles(_ bundleID: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    guard let bundleID = bundleID, AXIsProcessTrusted() else { return jsonStrings([]) }
    var titles: [String] = []
    for app in NSRunningApplication.runningApplications(
        withBundleIdentifier: String(cString: bundleID))
    {
        let element = AXUIElementCreateApplication(app.processIdentifier)
        // A busy app must not stall the poll.
        AXUIElementSetMessagingTimeout(element, 0.25)
        var windows: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, kAXWindowsAttribute as CFString, &windows)
            == .success, let list = windows as? [AXUIElement]
        else { continue }
        for window in list {
            var title: CFTypeRef?
            if AXUIElementCopyAttributeValue(window, kAXTitleAttribute as CFString, &title)
                == .success, let title = title as? String, !title.isEmpty
            {
                titles.append(title)
            }
        }
    }
    return jsonStrings(titles)
}
