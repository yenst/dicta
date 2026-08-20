use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct PosterPlan {
    program: PathBuf,
    args: Vec<OsString>,
    staging: PathBuf,
}

trait PosterExecutor {
    fn run(&self, plan: &PosterPlan) -> io::Result<bool>;
}

struct SystemPosterExecutor;

impl PosterExecutor for SystemPosterExecutor {
    fn run(&self, plan: &PosterPlan) -> io::Result<bool> {
        Command::new(&plan.program)
            .args(&plan.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
    }
}

pub(crate) fn extract(video: &Path) -> Option<PathBuf> {
    extract_with(video, &SystemPosterExecutor)
}

fn extract_with(video: &Path, executor: &impl PosterExecutor) -> Option<PathBuf> {
    if !is_regular_file(video) {
        return None;
    }
    let poster = video.with_extension("poster.jpg");
    if poster.exists() || fs::symlink_metadata(&poster).is_ok() {
        return is_regular_file(&poster).then_some(poster);
    }
    let staging = video.with_extension("poster.part.jpg");
    if staging.exists() || fs::symlink_metadata(&staging).is_ok() {
        return None;
    }
    let plan = plan(video, &staging);
    if !executor.run(&plan).ok()? || !is_regular_nonempty_file(&staging) {
        let _ = fs::remove_file(&staging);
        return None;
    }
    if let Ok(file) = fs::File::open(&staging) {
        let _ = file.sync_all();
    }
    if fs::hard_link(&staging, &poster).is_err() {
        let _ = fs::remove_file(&staging);
        return None;
    }
    let _ = fs::remove_file(&staging);
    Some(poster)
}

fn plan(video: &Path, staging: &Path) -> PosterPlan {
    PosterPlan {
        program: PathBuf::from("ffmpeg"),
        args: [
            OsString::from("-nostdin"),
            OsString::from("-hide_banner"),
            OsString::from("-loglevel"),
            OsString::from("error"),
            OsString::from("-y"),
            OsString::from("-ss"),
            OsString::from("0.1"),
            OsString::from("-i"),
            video.as_os_str().to_owned(),
            OsString::from("-frames:v"),
            OsString::from("1"),
            OsString::from("-q:v"),
            OsString::from("3"),
            staging.as_os_str().to_owned(),
        ]
        .into_iter()
        .collect(),
        staging: staging.to_path_buf(),
    }
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn is_regular_nonempty_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() > 0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeExecutor {
        plans: RefCell<Vec<PosterPlan>>,
        succeed: bool,
    }

    impl PosterExecutor for FakeExecutor {
        fn run(&self, plan: &PosterPlan) -> io::Result<bool> {
            self.plans.borrow_mut().push(plan.clone());
            if self.succeed {
                fs::write(&plan.staging, b"jpeg")?;
            }
            Ok(self.succeed)
        }
    }

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dicta-poster-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn poster_extraction_is_shell_free_atomic_and_no_clobber() {
        let root = root("success");
        fs::create_dir_all(&root).unwrap();
        let video = root.join("recording with spaces.mp4");
        fs::write(&video, b"video").unwrap();
        let executor = FakeExecutor {
            plans: RefCell::new(Vec::new()),
            succeed: true,
        };

        let poster = extract_with(&video, &executor).unwrap();
        assert_eq!(poster, video.with_extension("poster.jpg"));
        assert_eq!(fs::read(&poster).unwrap(), b"jpeg");
        assert!(!video.with_extension("poster.part.jpg").exists());
        let plans = executor.plans.borrow();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].program, Path::new("ffmpeg"));
        assert_eq!(plans[0].args[8], video.as_os_str());
        drop(plans);

        fs::write(&poster, b"keep").unwrap();
        assert_eq!(extract_with(&video, &executor), Some(poster.clone()));
        assert_eq!(fs::read(&poster).unwrap(), b"keep");
        assert_eq!(executor.plans.borrow().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_or_symlinked_outputs_are_cleaned_without_overwrite() {
        let root = root("failure");
        fs::create_dir_all(&root).unwrap();
        let video = root.join("recording.mp4");
        fs::write(&video, b"video").unwrap();
        let executor = FakeExecutor {
            plans: RefCell::new(Vec::new()),
            succeed: false,
        };
        assert_eq!(extract_with(&video, &executor), None);
        assert!(!video.with_extension("poster.part.jpg").exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = root.join("outside.jpg");
            fs::write(&outside, b"outside").unwrap();
            symlink(&outside, video.with_extension("poster.jpg")).unwrap();
            assert_eq!(extract_with(&video, &executor), None);
            assert_eq!(fs::read(outside).unwrap(), b"outside");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn system_ffmpeg_extracts_a_real_jpeg_frame() {
        if !Command::new("ffmpeg")
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
        {
            return;
        }
        let root = root("real");
        fs::create_dir_all(&root).unwrap();
        let video = root.join("fixture.mp4");
        let generated = Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x7aa2f7:s=320x180:d=0.4",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&video)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(generated.success());

        let poster = extract(&video).expect("system ffmpeg extracts the poster");
        let jpeg = fs::read(&poster).unwrap();
        assert!(jpeg.starts_with(&[0xff, 0xd8]));
        assert!(jpeg.ends_with(&[0xff, 0xd9]));
        assert!(!video.with_extension("poster.part.jpg").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
