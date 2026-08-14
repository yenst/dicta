#import "DictaNative.h"

#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <CoreMedia/CoreMedia.h>
#import <Foundation/Foundation.h>
#import <math.h>

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
