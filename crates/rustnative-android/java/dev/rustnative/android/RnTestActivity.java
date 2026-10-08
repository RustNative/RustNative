package dev.rustnative.android;

/**
 * The activity the device suite runs in. Unlike {@link RnActivity} as a
 * launcher, it does not start the application's `main`: each test realizes
 * its own application in it (`device_tests::harness`).
 */
public class RnTestActivity extends RnActivity {
}
