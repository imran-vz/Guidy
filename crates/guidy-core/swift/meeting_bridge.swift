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

private final class MicCapture {
    private let engine = AVAudioEngine()
    private let sink: CaptureSink
    /// Serializes restarts and stop.
    private let controlQueue = DispatchQueue(label: "dev.guidy.meetings.mic.control")
    private let stateLock = NSLock()
    private var resampler: MonoResampler?
    private var observer: NSObjectProtocol?
    private var stopped = false

    init(sink: CaptureSink) {
        self.sink = sink
    }

    func start() throws {
        do {
            try startVoiceProcessing()
        } catch {
            // Echo cancellation is preferred, not required: record the plain
            // mic rather than nothing, and say so.
            engine.stop()
            engine.inputNode.removeTap(onBus: 0)
            try? engine.inputNode.setVoiceProcessingEnabled(false)
            try installTapAndStart()
            sink.event(
                GUIDY_MB_STREAM_MIC,
                "echo cancellation unavailable, recording the plain microphone: \(error)")
        }

        observer = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange, object: engine, queue: nil
        ) { [weak self] _ in
            guard let self = self else { return }
            self.controlQueue.async { self.restart() }
        }
    }

    private func startVoiceProcessing() throws {
        let input = engine.inputNode
        do {
            try input.setVoiceProcessingEnabled(true)
        } catch {
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

        try installTapAndStart()
    }

    private func installTapAndStart() throws {
        let input = engine.inputNode
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

        input.installTap(onBus: 0, bufferSize: 4096, format: format) { [weak self] buffer, _ in
            self?.handle(buffer)
        }
        engine.prepare()
        do {
            try engine.start()
        } catch {
            input.removeTap(onBus: 0)
            throw BridgeError("starting the microphone failed: \(error)")
        }
    }

    private func handle(_ buffer: AVAudioPCMBuffer) {
        stateLock.lock()
        let resampler = self.resampler
        stateLock.unlock()
        guard let resampler = resampler else { return }
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
        resampler.process(mono) { samples, count in
            sink.audio(GUIDY_MB_STREAM_MIC, samples, count)
        }
    }

    /// The input or output device changed: rebuild the tap with the new format.
    private func restart() {
        if stopped { return }
        engine.stop()
        engine.inputNode.removeTap(onBus: 0)
        do {
            try installTapAndStart()
            sink.event(GUIDY_MB_STREAM_MIC, "microphone restarted after an audio device change")
        } catch {
            sink.event(GUIDY_MB_STREAM_MIC, "error: \(error)")
        }
    }

    func stop() {
        if let observer = observer {
            NotificationCenter.default.removeObserver(observer)
            self.observer = nil
        }
        controlQueue.sync {
            stopped = true
            engine.stop()
            engine.inputNode.removeTap(onBus: 0)
        }
        // Leave the shared input node in its normal mode for dictation.
        try? engine.inputNode.setVoiceProcessingEnabled(false)
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

/// Keeps the system stream continuous and on the wall clock. A process tap
/// delivers nothing while no process plays audio, so gaps are filled with
/// silence; sample `n` of the stream is then (within `tolerance`) captured
/// `n / 16 kHz` seconds after capture start, like the mic stream.
private final class TimelineAligner {
    private let startNanos: UInt64
    private var emitted: Int64 = 0
    private var lastAudioNanos: UInt64
    /// 100 ms: jitter and converter latency below this are left alone.
    private let tolerance: Int64 = 1_600
    private let zeros = [Float](repeating: 0, count: 1_600)

    init(startNanos: UInt64) {
        self.startNanos = startNanos
        self.lastAudioNanos = startNanos
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

    /// Audio whose first sample was captured at `nanos`.
    func audio(
        _ samples: UnsafePointer<Float>, _ count: Int, atNanos nanos: UInt64,
        _ emit: (UnsafePointer<Float>, Int) -> Void
    ) {
        lastAudioNanos = max(lastAudioNanos, nanos)
        let target = position(atNanos: nanos)
        var skip = 0
        if target > emitted + tolerance {
            padSilence(to: target, emit)
        } else if target < emitted - tolerance {
            // Silence was padded past this point already: drop the overlap.
            skip = Int(min(Int64(count), emitted - target))
        }
        if skip < count {
            emit(samples + skip, count - skip)
            emitted += Int64(count - skip)
        }
    }

    /// Called periodically: fill a silence that has lasted a while.
    func tick(_ emit: (UnsafePointer<Float>, Int) -> Void) {
        let now = nowNanos()
        guard now > lastAudioNanos + 300_000_000 else { return }
        padSilence(to: position(atNanos: now - 100_000_000), emit)
    }
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

    init(sink: CaptureSink?) {
        self.sink = sink
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
        let aligner = TimelineAligner(startNanos: nowNanos())
        ioQueue.sync { self.aligner = aligner }
        let timer = DispatchSource.makeTimerSource(queue: ioQueue)
        timer.schedule(deadline: .now() + .milliseconds(100), repeating: .milliseconds(100))
        timer.setEventHandler {
            aligner.tick { samples, count in sink.audio(GUIDY_MB_STREAM_SYSTEM, samples, count) }
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
        let capturedAt =
            time.mFlags.contains(.hostTimeValid) ? hostNanos(time.mHostTime) : nowNanos()
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

    if wantMic != 0 {
        let mic = MicCapture(sink: session.sink)
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
        let system = SystemTapCapture(sink: session.sink)
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
// the same approach as Apple's own sample apps' community ports); without them
// a short-lived tap triggers the prompt.
private typealias TCCPreflightFn = @convention(c) (CFString, CFDictionary?) -> Int32
private typealias TCCRequestFn = @convention(c) (
    CFString, CFDictionary?, @escaping @convention(block) (Bool) -> Void
) -> Void

private let tccHandle: UnsafeMutableRawPointer? = dlopen(
    "/System/Library/PrivateFrameworks/TCC.framework/Versions/A/TCC", RTLD_NOW)
private let audioCaptureService = "kTCCServiceAudioCapture" as CFString

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

@_cdecl("guidy_mb_system_audio_status")
public func guidyMbSystemAudioStatus() -> Int32 {
    guard #available(macOS 14.2, *) else { return GUIDY_MB_AUTH_UNAVAILABLE }
    guard let preflight = tccPreflight() else { return GUIDY_MB_AUTH_UNKNOWN }
    switch preflight(audioCaptureService, nil) {
    case 0: return GUIDY_MB_AUTH_GRANTED
    case 1: return GUIDY_MB_AUTH_DENIED
    default: return GUIDY_MB_AUTH_NOT_DETERMINED
    }
}

@_cdecl("guidy_mb_request_system_audio_access")
public func guidyMbRequestSystemAudioAccess() -> Int32 {
    guard #available(macOS 14.2, *) else { return GUIDY_MB_AUTH_UNAVAILABLE }
    let current = guidyMbSystemAudioStatus()
    if current == GUIDY_MB_AUTH_GRANTED || current == GUIDY_MB_AUTH_DENIED {
        return current
    }
    if isBundled() && !hasUsageDescription("NSAudioCaptureUsageDescription") {
        NSLog("[guidy meetings] Info.plist lacks NSAudioCaptureUsageDescription")
        return GUIDY_MB_AUTH_DENIED
    }

    if let request = tccRequest() {
        let semaphore = DispatchSemaphore(value: 0)
        let granted = Box(false)
        request(audioCaptureService, nil) { ok in
            granted.value = ok
            semaphore.signal()
        }
        if semaphore.wait(timeout: .now() + 300) == .success {
            return granted.value ? GUIDY_MB_AUTH_GRANTED : GUIDY_MB_AUTH_DENIED
        }
        return guidyMbSystemAudioStatus()
    }

    // Fallback probe: a short tap makes the system ask. Then wait (bounded) for
    // the answer when the status can be read at all.
    let probe = SystemTapCapture(sink: nil)
    do {
        try probe.start()
    } catch {
        NSLog("[guidy meetings] system audio probe failed: %@", "\(error)")
        return guidyMbSystemAudioStatus()
    }
    Thread.sleep(forTimeInterval: 1.0)
    var status = guidyMbSystemAudioStatus()
    let deadline = Date().addingTimeInterval(120)
    while status == GUIDY_MB_AUTH_NOT_DETERMINED && Date() < deadline {
        Thread.sleep(forTimeInterval: 0.25)
        status = guidyMbSystemAudioStatus()
    }
    probe.stop()
    return status
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
