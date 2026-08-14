#import <Foundation/Foundation.h>
#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <Speech/Speech.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreVideo/CoreVideo.h>
#import <CoreGraphics/CoreGraphics.h>
#import <math.h>

typedef void (*RecorderCallback)(const char *event, const char *message);

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

@interface DictaTranscriptionJob : NSObject
@property(nonatomic, copy) NSString *inputPath;
@property(nonatomic, copy) NSString *language;
@property(nonatomic, assign) RecorderCallback callback;
@end

@implementation DictaTranscriptionJob
@end

@interface DictaTranscriber : NSObject
@property(nonatomic, strong) NSMutableArray<DictaTranscriptionJob *> *queue;
@property(nonatomic, strong) DictaTranscriptionJob *current;
@property(nonatomic, strong) SFSpeechRecognizer *recognizer;
@property(nonatomic, strong) SFSpeechRecognitionTask *task;
@property(nonatomic, copy) NSString *latestTranscript;
@end

@implementation DictaTranscriber

- (instancetype)init {
    self = [super init];
    if (self) self.queue = [NSMutableArray array];
    return self;
}

- (NSString *)payloadForPath:(NSString *)path value:(NSString *)value key:(NSString *)key {
    NSDictionary *payload = @{ @"path": path ?: @"", key: value ?: @"" };
    NSData *data = [NSJSONSerialization dataWithJSONObject:payload options:0 error:nil];
    return data == nil ? @"{}" : [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
}

- (void)enqueuePath:(NSString *)path language:(NSString *)language callback:(RecorderCallback)callback {
    if (path.length == 0) return;
    DictaTranscriptionJob *job = [[DictaTranscriptionJob alloc] init];
    job.inputPath = path;
    job.language = language.length > 0 ? language : @"auto";
    job.callback = callback;
    [self.queue addObject:job];
    [self startNextIfNeeded];
}

- (void)startNextIfNeeded {
    if (self.current != nil || self.queue.count == 0) return;
    self.current = self.queue.firstObject;
    [self.queue removeObjectAtIndex:0];
    Emit(self.current.callback, @"transcribing", [self payloadForPath:self.current.inputPath value:@"" key:@"message"]);

    [SFSpeechRecognizer requestAuthorization:^(SFSpeechRecognizerAuthorizationStatus status) {
        dispatch_async(dispatch_get_main_queue(), ^{
            if (status != SFSpeechRecognizerAuthorizationStatusAuthorized) {
                [self finishWithError:@"Speech Recognition permission was denied. Enable Dicta in System Settings → Privacy & Security → Speech Recognition."];
                return;
            }
            [self beginRecognition];
        });
    }];
}

- (NSLocale *)localeForLanguage:(NSString *)language {
    static NSDictionary<NSString *, NSString *> *identifiers;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{
        identifiers = @{
            @"nl": @"nl-NL",
            @"en": @"en-US",
            @"fr": @"fr-FR",
            @"de": @"de-DE",
            @"es": @"es-ES"
        };
    });
    NSString *identifier = identifiers[language];
    if (identifier.length == 0) return NSLocale.currentLocale;
    NSLocale *locale = [NSLocale localeWithLocaleIdentifier:identifier];
    return locale ?: NSLocale.currentLocale;
}

- (void)beginRecognition {
    self.recognizer = [[SFSpeechRecognizer alloc] initWithLocale:[self localeForLanguage:self.current.language]];
    if (self.recognizer == nil || !self.recognizer.available) {
        [self finishWithError:@"macOS Speech Recognition is currently unavailable."];
        return;
    }

    NSURL *url = [NSURL fileURLWithPath:self.current.inputPath];
    SFSpeechURLRecognitionRequest *request = [[SFSpeechURLRecognitionRequest alloc] initWithURL:url];
    request.shouldReportPartialResults = YES;
    request.taskHint = SFSpeechRecognitionTaskHintDictation;
    if (self.recognizer.supportsOnDeviceRecognition) request.requiresOnDeviceRecognition = YES;
    self.latestTranscript = @"";

    __weak DictaTranscriber *weakSelf = self;
    self.task = [self.recognizer recognitionTaskWithRequest:request
                                             resultHandler:^(SFSpeechRecognitionResult *result, NSError *error) {
        dispatch_async(dispatch_get_main_queue(), ^{
            DictaTranscriber *strongSelf = weakSelf;
            if (strongSelf == nil || strongSelf.current == nil) return;
            if (result != nil) strongSelf.latestTranscript = result.bestTranscription.formattedString ?: @"";
            if (result.isFinal) {
                if (strongSelf.latestTranscript.length == 0) {
                    [strongSelf finishWithError:@"No speech was detected in this recording."];
                } else {
                    [strongSelf finishWithTranscript:strongSelf.latestTranscript];
                }
            } else if (error != nil) {
                [strongSelf finishWithError:error.localizedDescription ?: @"The recording could not be transcribed."];
            }
        });
    }];
}

- (void)finishWithTranscript:(NSString *)transcript {
    if (self.current == nil) return;
    RecorderCallback callback = self.current.callback;
    NSString *payload = [self payloadForPath:self.current.inputPath value:transcript key:@"transcript"];
    [self resetCurrent];
    Emit(callback, @"transcript", payload);
    [self startNextIfNeeded];
}

- (void)finishWithError:(NSString *)error {
    if (self.current == nil) return;
    RecorderCallback callback = self.current.callback;
    NSString *payload = [self payloadForPath:self.current.inputPath value:error key:@"error"];
    [self resetCurrent];
    Emit(callback, @"transcription_error", payload);
    [self startNextIfNeeded];
}

- (void)resetCurrent {
    [self.task cancel];
    self.task = nil;
    self.recognizer = nil;
    self.latestTranscript = nil;
    self.current = nil;
}

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

static DictaTranscriber *SharedTranscriber(void) {
    static DictaTranscriber *transcriber;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{ transcriber = [[DictaTranscriber alloc] init]; });
    return transcriber;
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

void dicta_transcribe(const char *input_path, const char *language, RecorderCallback callback) {
    NSString *path = [NSString stringWithUTF8String:input_path];
    NSString *spoken = language == NULL ? @"auto" : [NSString stringWithUTF8String:language];
    dispatch_async(dispatch_get_main_queue(), ^{
        [SharedTranscriber() enqueuePath:path language:spoken callback:callback];
    });
}

bool dicta_extract_audio(const char *input_path, const char *output_path) {
    @autoreleasepool {
        NSError *error = nil;
        NSURL *inputURL = [NSURL fileURLWithPath:[NSString stringWithUTF8String:input_path]];
        NSURL *outputURL = [NSURL fileURLWithPath:[NSString stringWithUTF8String:output_path]];
        [[NSFileManager defaultManager] removeItemAtURL:outputURL error:nil];

        AVAudioFile *input = [[AVAudioFile alloc] initForReading:inputURL error:&error];
        if (input == nil || error != nil || input.length == 0) return false;
        AVAudioFormat *sourceFormat = input.processingFormat;
        AVAudioFormat *targetFormat = [[AVAudioFormat alloc]
            initWithCommonFormat:AVAudioPCMFormatInt16
                     sampleRate:16000
                       channels:1
                    interleaved:YES];
        AVAudioConverter *converter = [[AVAudioConverter alloc] initFromFormat:sourceFormat
                                                                      toFormat:targetFormat];
        if (converter == nil) return false;

        AVAudioFile *output = [[AVAudioFile alloc]
            initForWriting:outputURL
                   settings:targetFormat.settings
               commonFormat:AVAudioPCMFormatInt16
                interleaved:YES
                      error:&error];
        if (output == nil || error != nil) return false;

        const AVAudioFrameCount inChunk = 16384;
        double ratio = targetFormat.sampleRate / sourceFormat.sampleRate;
        AVAudioFrameCount outChunk = (AVAudioFrameCount)ceil(inChunk * ratio) + 64;
        AVAudioPCMBuffer *source = [[AVAudioPCMBuffer alloc]
            initWithPCMFormat:sourceFormat
                frameCapacity:inChunk];
        AVAudioPCMBuffer *target = [[AVAudioPCMBuffer alloc]
            initWithPCMFormat:targetFormat
                frameCapacity:outChunk];
        BOOL wroteAny = NO;

        while (input.framePosition < input.length) {
            AVAudioFrameCount remaining = (AVAudioFrameCount)(input.length - input.framePosition);
            source.frameLength = MIN(inChunk, remaining);
            if (![input readIntoBuffer:source error:&error] || error != nil) return false;

            __block BOOL suppliedInput = NO;
            AVAudioConverterOutputStatus status = [converter
                convertToBuffer:target
                          error:&error
             withInputFromBlock:^AVAudioBuffer *(AVAudioPacketCount requestedPackets,
                                                 AVAudioConverterInputStatus *inputStatus) {
                (void)requestedPackets;
                if (suppliedInput) {
                    *inputStatus = AVAudioConverterInputStatus_NoDataNow;
                    return nil;
                }
                suppliedInput = YES;
                *inputStatus = AVAudioConverterInputStatus_HaveData;
                return source;
            }];
            if (status == AVAudioConverterOutputStatus_Error || error != nil) return false;
            if (target.frameLength > 0) {
                if (![output writeFromBuffer:target error:&error] || error != nil) return false;
                wroteAny = YES;
            }
        }

        __block BOOL flushed = NO;
        AVAudioConverterOutputStatus flushStatus = [converter
            convertToBuffer:target
                      error:&error
         withInputFromBlock:^AVAudioBuffer *(AVAudioPacketCount requestedPackets,
                                             AVAudioConverterInputStatus *inputStatus) {
            (void)requestedPackets;
            if (flushed) {
                *inputStatus = AVAudioConverterInputStatus_EndOfStream;
                return nil;
            }
            flushed = YES;
            *inputStatus = AVAudioConverterInputStatus_EndOfStream;
            return nil;
        }];
        if (flushStatus != AVAudioConverterOutputStatus_Error && error == nil && target.frameLength > 0) {
            if (![output writeFromBuffer:target error:&error] || error != nil) return false;
            wroteAny = YES;
        }

        return wroteAny;
    }
}

bool dicta_extract_poster(const char *input_path, const char *output_path) {
    @autoreleasepool {
        if (input_path == NULL || output_path == NULL) return false;
        NSString *input = [NSString stringWithUTF8String:input_path];
        NSString *output = [NSString stringWithUTF8String:output_path];
        if (input.length == 0 || output.length == 0) return false;

        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:input] options:nil];
        AVAssetImageGenerator *generator = [[AVAssetImageGenerator alloc] initWithAsset:asset];
        generator.appliesPreferredTrackTransform = YES;
        generator.maximumSize = CGSizeMake(640.0, 400.0);
        generator.requestedTimeToleranceBefore = CMTimeMakeWithSeconds(0.35, 600);
        generator.requestedTimeToleranceAfter = CMTimeMakeWithSeconds(0.35, 600);

        CMTime requested = CMTimeMakeWithSeconds(0.8, 600);
        NSError *error = nil;
        CGImageRef image = [generator copyCGImageAtTime:requested actualTime:NULL error:&error];
        if (image == NULL || error != nil) {
            if (image != NULL) CGImageRelease(image);
            return false;
        }

        NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc] initWithCGImage:image];
        CGImageRelease(image);
        NSData *jpeg = [bitmap representationUsingType:NSBitmapImageFileTypeJPEG
                                            properties:@{ NSImageCompressionFactor: @0.72 }];
        return jpeg != nil && [jpeg writeToFile:output atomically:YES];
    }
}
