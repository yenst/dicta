#import "DictaNative.h"

#import <Foundation/Foundation.h>
#import <Speech/Speech.h>

static void Emit(RecorderCallback callback, NSString *event, NSString *message) {
    if (callback != NULL) {
        callback(event.UTF8String, (message ?: @"").UTF8String);
    }
}

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
@property(nonatomic, copy) NSArray<NSDictionary *> *latestSegments;
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

- (NSArray<NSDictionary *> *)segmentsForTranscription:(SFTranscription *)transcription {
    NSMutableArray<NSDictionary *> *segments = [NSMutableArray array];
    NSCharacterSet *whitespace = NSCharacterSet.whitespaceAndNewlineCharacterSet;
    for (SFTranscriptionSegment *segment in transcription.segments) {
        NSString *text = [segment.substring stringByTrimmingCharactersInSet:whitespace];
        if (text.length == 0) continue;
        NSTimeInterval start = MAX(0.0, segment.timestamp);
        NSTimeInterval end = MAX(start, segment.timestamp + segment.duration);
        [segments addObject:@{
            @"start_seconds": @(start),
            @"end_seconds": @(end),
            @"text": text
        }];
    }
    return segments;
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
    self.latestSegments = @[];

    __weak DictaTranscriber *weakSelf = self;
    self.task = [self.recognizer recognitionTaskWithRequest:request
                                             resultHandler:^(SFSpeechRecognitionResult *result, NSError *error) {
        dispatch_async(dispatch_get_main_queue(), ^{
            DictaTranscriber *strongSelf = weakSelf;
            if (strongSelf == nil || strongSelf.current == nil) return;
            if (result != nil) {
                strongSelf.latestTranscript = result.bestTranscription.formattedString ?: @"";
                strongSelf.latestSegments = [strongSelf segmentsForTranscription:result.bestTranscription];
            }
            if (result.isFinal) {
                if (strongSelf.latestTranscript.length == 0) {
                    [strongSelf finishWithError:@"No speech was detected in this recording."];
                } else {
                    [strongSelf finishWithTranscript:strongSelf.latestTranscript segments:strongSelf.latestSegments];
                }
            } else if (error != nil) {
                [strongSelf finishWithError:error.localizedDescription ?: @"The recording could not be transcribed."];
            }
        });
    }];
}

- (void)finishWithTranscript:(NSString *)transcript segments:(NSArray<NSDictionary *> *)segments {
    if (self.current == nil) return;
    RecorderCallback callback = self.current.callback;
    NSDictionary *payloadObject = @{
        @"path": self.current.inputPath ?: @"",
        @"transcript": transcript ?: @"",
        @"transcript_segments": segments ?: @[]
    };
    NSData *payloadData = [NSJSONSerialization dataWithJSONObject:payloadObject options:0 error:nil];
    NSString *payload = payloadData == nil
        ? @"{}"
        : [[NSString alloc] initWithData:payloadData encoding:NSUTF8StringEncoding];
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
    self.latestSegments = nil;
    self.current = nil;
}

@end

static DictaTranscriber *SharedTranscriber(void) {
    static DictaTranscriber *transcriber;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{ transcriber = [[DictaTranscriber alloc] init]; });
    return transcriber;
}

void dicta_transcribe(const char *input_path, const char *language, RecorderCallback callback) {
    NSString *path = [NSString stringWithUTF8String:input_path];
    NSString *spoken = language == NULL ? @"auto" : [NSString stringWithUTF8String:language];
    dispatch_async(dispatch_get_main_queue(), ^{
        [SharedTranscriber() enqueuePath:path language:spoken callback:callback];
    });
}
