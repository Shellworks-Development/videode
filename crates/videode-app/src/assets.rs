//! Asset source: GPUI Kit's default icons plus the extra Lucide icons the
//! editor uses. Only the listed icons are embedded.

use std::borrow::Cow;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    EditorIcons,
    [
        Film,
        Import,
        Upload,
        Scissors,
        Magnet,
        MousePointer,
        Layers,
        Workflow,
        SkipBack,
        SkipForward,
        StepBack,
        StepForward,
        Play,
        Pause,
        Keyboard,
        Volume2,
        VolumeX,
        Eye,
        EyeOff,
        Lock,
        LockOpen,
        Link,
        Power,
        Trash,
        Code,
    ]
);

#[derive(Clone, Copy, Debug, Default)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match EditorIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Assets.list(path)
    }
}
