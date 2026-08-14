#import "DictaNative.h"

#import <AVFoundation/AVFoundation.h>
#import <CoreGraphics/CoreGraphics.h>
#import <CoreVideo/CoreVideo.h>
#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>

static void Emit(RecorderCallback callback, NSString *event, NSString *message) {
    if (callback != NULL) {
        callback(event.UTF8String, (message ?: @"").UTF8String);
    }
}

API_AVAILABLE(macos(15.0))
@interface DictaRecorder : NSObject <SCStreamDelegate, SCRecordingOutputDelegate>
@property(nonatomic, strong) SCStream *stream;
@property(nonatomic, strong) SCRecordingOutput *recordingOutput;
@property(nonatomic, copy) NSString *outputPath;
@property(nonatomic, assign) RecorderCallback callback;
@property(nonatomic, assign) BOOL recording;
@property(nonatomic, assign) BOOL stopping;
@end

@implementation DictaRecorder

- (void)startAtPath:(NSString *)path callback:(RecorderCallback)callback {
    self.callback = callback;
    self.outputPath = path;

    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
        dispatch_async(dispatch_get_main_queue(), ^{
            if (!granted) {
                Emit(self.callback, @"error", @"Microphone permission was denied. Enable it in System Settings → Privacy & Security → Microphone.");
                return;
            }
            [self prepareScreenCapture];
        });
    }];
}

- (void)prepareScreenCapture {
    // Ad-hoc rebuilds change the CDHash, so TCC often keeps a stale grant and
    // SCShareableContent fails without showing the Allow dialog. This is the
    // API that actually prompts.
    if (!CGPreflightScreenCaptureAccess()) {
        CGRequestScreenCaptureAccess();
    }

    [SCShareableContent getShareableContentExcludingDesktopWindows:NO
                                              onScreenWindowsOnly:YES
                                               completionHandler:^(SCShareableContent *content, NSError *error) {
        dispatch_async(dispatch_get_main_queue(), ^{
            if (error != nil) {
                Emit(self.callback, @"error", [self readableError:error fallback:@"Screen Recording permission is required. Enable it in System Settings → Privacy & Security → Screen & System Audio Recording."]);
                return;
            }
            CGDirectDisplayID mainDisplayID = CGMainDisplayID();
            SCDisplay *display = nil;
            for (SCDisplay *candidate in content.displays) {
                if (candidate.displayID == mainDisplayID) {
                    display = candidate;
                    break;
                }
            }
            if (display == nil) display = content.displays.firstObject;
            if (display == nil) {
                Emit(self.callback, @"error", @"No display is available to record.");
                return;
            }

            SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];
            SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
            config.width = display.width;
            config.height = display.height;
            config.minimumFrameInterval = CMTimeMake(1, 30);
            config.queueDepth = 6;
            config.pixelFormat = kCVPixelFormatType_32BGRA;
            config.showsCursor = YES;
            config.showMouseClicks = YES;
            config.capturesAudio = YES;
            config.captureMicrophone = YES;
            config.excludesCurrentProcessAudio = YES;
            config.sampleRate = 48000;
            config.channelCount = 2;

            SCRecordingOutputConfiguration *recordingConfig = [[SCRecordingOutputConfiguration alloc] init];
            recordingConfig.outputURL = [NSURL fileURLWithPath:self.outputPath];
            recordingConfig.videoCodecType = AVVideoCodecTypeH264;
            recordingConfig.outputFileType = AVFileTypeMPEG4;

            self.stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:self];
            self.recordingOutput = [[SCRecordingOutput alloc] initWithConfiguration:recordingConfig delegate:self];

            NSError *addError = nil;
            if (![self.stream addRecordingOutput:self.recordingOutput error:&addError]) {
                Emit(self.callback, @"error", [self readableError:addError fallback:@"Could not attach the recording output."]);
                [self reset];
                return;
            }

            [self.stream startCaptureWithCompletionHandler:^(NSError *startError) {
                if (startError != nil) {
                    Emit(self.callback, @"error", [self readableError:startError fallback:@"Could not start screen capture."]);
                    [self reset];
                }
            }];
        });
    }];
}

- (void)stopWithCallback:(RecorderCallback)callback {
    if (callback != NULL) self.callback = callback;
    if (self.stream == nil || self.stopping) return;
    self.stopping = YES;
    [self.stream stopCaptureWithCompletionHandler:^(NSError *error) {
        if (error != nil) {
            Emit(self.callback, @"error", [self readableError:error fallback:@"Could not finish the recording."]);
            [self reset];
        }
    }];
}

- (void)recordingOutputDidStartRecording:(SCRecordingOutput *)recordingOutput {
    self.recording = YES;
    Emit(self.callback, @"started", self.outputPath);
}

- (void)recordingOutput:(SCRecordingOutput *)recordingOutput didFailWithError:(NSError *)error {
    Emit(self.callback, @"error", [self readableError:error fallback:@"The recording failed."]);
    [self reset];
}

- (void)recordingOutputDidFinishRecording:(SCRecordingOutput *)recordingOutput {
    Emit(self.callback, @"finished", self.outputPath);
    [self reset];
}

- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    if (!self.stopping) {
        Emit(self.callback, @"error", [self readableError:error fallback:@"Screen capture stopped unexpectedly."]);
        [self reset];
    }
}

- (NSString *)readableError:(NSError *)error fallback:(NSString *)fallback {
    if (error == nil) return fallback;
    NSString *description = error.localizedDescription;
    return description.length > 0 ? description : fallback;
}

- (void)reset {
    self.recording = NO;
    self.stopping = NO;
    self.recordingOutput = nil;
    self.stream = nil;
    self.outputPath = nil;
}

@end

static DictaRecorder *SharedRecorder(void) API_AVAILABLE(macos(15.0));
static DictaRecorder *SharedRecorder(void) {
    static DictaRecorder *recorder;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{ recorder = [[DictaRecorder alloc] init]; });
    return recorder;
}

void dicta_start(const char *output_path, RecorderCallback callback) {
    if (@available(macOS 15.0, *)) {
        NSString *path = [NSString stringWithUTF8String:output_path];
        dispatch_async(dispatch_get_main_queue(), ^{ [SharedRecorder() startAtPath:path callback:callback]; });
    } else {
        Emit(callback, @"error", @"Dicta requires macOS 15 or newer.");
    }
}

void dicta_stop(RecorderCallback callback) {
    if (@available(macOS 15.0, *)) {
        dispatch_async(dispatch_get_main_queue(), ^{ [SharedRecorder() stopWithCallback:callback]; });
    }
}
