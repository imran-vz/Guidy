// Catches Objective-C exceptions for `meeting_bridge.swift`. Swift cannot
// catch NSException, and AVAudioEngine reports some failures that way (e.g.
// `installTapOnBus` with a format that no longer matches the hardware after a
// device change), which otherwise aborts the whole app mid-meeting.

#import <Foundation/Foundation.h>
#include <stdlib.h>
#include <string.h>

int guidy_mb_objc_try(void (^NS_NOESCAPE block)(void), char **reason) {
    @try {
        block();
        return 0;
    } @catch (NSException *exception) {
        if (reason != NULL) {
            NSString *text = [NSString
                stringWithFormat:@"%@: %@", exception.name, exception.reason ?: @"(no reason)"];
            *reason = strdup(text.UTF8String ?: "Objective-C exception");
        }
        return 1;
    } @catch (id other) {
        if (reason != NULL) {
            *reason = strdup("unknown Objective-C exception");
        }
        return 1;
    }
}
