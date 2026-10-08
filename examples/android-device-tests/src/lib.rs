//! The Android backend's device suite, as an application: its instrumentation
//! (`dev.rustnative.android.RnInstrumentation`) runs the suite in the test
//! activity. Run it with `tools/android-device-test.sh`.

// No `main`: each test realizes its own application in the test activity.
#[cfg(target_os = "android")]
rustnative_android::export_main!();
