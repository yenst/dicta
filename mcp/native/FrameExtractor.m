#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <CoreMedia/CoreMedia.h>

#include <stdbool.h>
#include <math.h>

bool dicta_mcp_extract_frame(const char *input_path,
                             double requested_seconds,
                             const char *output_path,
                             double *actual_seconds) {
    @autoreleasepool {
        if (input_path == NULL || output_path == NULL || !isfinite(requested_seconds)) {
            return false;
        }

        NSString *input = [NSString stringWithUTF8String:input_path];
        NSString *output = [NSString stringWithUTF8String:output_path];
        if (input == nil || output == nil) {
            return false;
        }

        AVURLAsset *asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:input]
                                                options:nil];
        AVAssetImageGenerator *generator = [[AVAssetImageGenerator alloc] initWithAsset:asset];
        generator.appliesPreferredTrackTransform = YES;
        generator.maximumSize = CGSizeMake(1440.0, 900.0);
        generator.requestedTimeToleranceBefore = CMTimeMakeWithSeconds(0.25, 600);
        generator.requestedTimeToleranceAfter = CMTimeMakeWithSeconds(0.25, 600);

        CMTime requested = CMTimeMakeWithSeconds(fmax(0.0, requested_seconds), 600);
        CMTime actual = kCMTimeInvalid;
        NSError *error = nil;
        CGImageRef image = [generator copyCGImageAtTime:requested
                                             actualTime:&actual
                                                  error:&error];
        if (image == NULL || error != nil) {
            if (image != NULL) {
                CGImageRelease(image);
            }
            return false;
        }

        NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc] initWithCGImage:image];
        CGImageRelease(image);
        NSData *jpeg = [bitmap representationUsingType:NSBitmapImageFileTypeJPEG
                                            properties:@{ NSImageCompressionFactor: @0.80 }];
        if (jpeg == nil || ![jpeg writeToFile:output atomically:YES]) {
            return false;
        }

        if (actual_seconds != NULL && CMTIME_IS_NUMERIC(actual)) {
            *actual_seconds = CMTimeGetSeconds(actual);
        }
        return true;
    }
}
