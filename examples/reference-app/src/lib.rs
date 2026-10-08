//! The reference application's Android entry (the desktop one is
//! `main.rs`): the layout conformance suite's reference screen, as the
//! Android backend realizes it.

#[cfg(target_os = "android")]
mod android {
    use rustnative_conformance::reference::{ReferenceScreen, Variant};
    use rustnative_core::{Application, Component, Platform, Size, Window};

    fn main() -> Result<(), rustnative_android::Error> {
        let mut application = Application::new(
            ReferenceScreen::new(Variant { pseudo: false, right_to_left: false }),
            Window::new("Reference application", Size::new(480, 600)),
        );
        rustnative_android::AndroidPlatform::new().run(&mut application)
    }

    rustnative_android::export_main!(main);
}
