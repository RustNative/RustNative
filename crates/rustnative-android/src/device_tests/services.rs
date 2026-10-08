//! The portable services against the device's own (Phase 6). Each runs on
//! the instrumentation thread, as a service called from a background task
//! does; what needs the main thread gets there through the services' own
//! posting, while the main looper keeps running.

use std::future::Future;
use std::time::Duration;

use rustnative_core::i18n::{Date, DateStyle, LocaleService};
use rustnative_core::industrial::{PrintJob, PrintService};
use rustnative_core::product::{CommerceService, PushService, SecureStorage};
use rustnative_core::{
    ClipboardService, FileDialogKind, FileDialogRequest, FileDialogService, HttpRequest,
    HttpService, Locale, Permission, PermissionService, PermissionState, StateStore, SystemService,
};
use rustnative_data::{Conditions, ImageDecoder};

use super::harness::{Instrumentation, java};
use crate::jni_host::{Arg, Class, call};
use crate::services::{
    AndroidClipboard, AndroidConditions, AndroidFileDialogs, AndroidHttp, AndroidLocale,
    AndroidPermissions, AndroidPrinting, AndroidPush, AndroidSecureStorage, AndroidStore,
    AndroidSystem, BitmapDecoder, FileStateStore, cache_dir,
};

fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}

pub(super) fn the_clipboard_round_trips(_: &Instrumentation) {
    block_on(AndroidClipboard.write_text("from the suite".into())).expect("written");
    let read = block_on(AndroidClipboard.read_text()).expect("read");
    assert_eq!(read.as_deref(), Some("from the suite"));
}

pub(super) fn a_url_nothing_opens_says_so(_: &Instrumentation) {
    let error =
        block_on(AndroidSystem::default().open_url("rustnative-nothing-handles-this://x".into()))
            .expect_err("unhandled");
    assert!(error.to_string().contains("nothing on this device opens"), "{error}");
}

pub(super) fn a_notification_posts_on_its_channel_with_its_actions(
    instrumentation: &Instrumentation,
) {
    let package = package();
    instrumentation.shell(&format!("pm grant {package} android.permission.POST_NOTIFICATIONS"));
    let system = AndroidSystem::on_channel("suite");
    let id = system
        .notify_with_actions("Suite", "A notification", &[("archive".into(), "Archive".into())])
        .expect("posted");
    let showing = || java(Class::Services, "notificationShowing", "(I)Z", &[Arg::Int(id)]).bool();
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while !showing() {
        assert!(std::time::Instant::now() < until, "the notification never showed");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        java(Class::Services, "channelExists", "(Ljava/lang/String;)Z", &[Arg::Str("suite")])
            .bool(),
        "the channel was created"
    );
    java(Class::Services, "cancelNotification", "(I)V", &[Arg::Int(id)]);
}

pub(super) fn permission_states_follow_the_system(instrumentation: &Instrumentation) {
    // Revoking a granted runtime permission kills the process (Android's
    // rule), so the suite reads the states it can reach without that: a
    // declared permission never asked for, then granted.
    let package = package();
    let before = AndroidPermissions.state(Permission::Camera).expect("state");
    assert!(matches!(before, PermissionState::NotAsked | PermissionState::Granted), "{before:?}");
    instrumentation.shell(&format!("pm grant {package} android.permission.CAMERA"));
    assert_eq!(
        AndroidPermissions.state(Permission::Camera).expect("state"),
        PermissionState::Granted
    );
    // A request for what is granted answers at once, without a prompt.
    assert_eq!(
        block_on(AndroidPermissions.request(Permission::Camera)).expect("answered"),
        PermissionState::Granted
    );
    // Undeclared in the manifest: never grantable, and not a crash.
    let contacts = AndroidPermissions.state(Permission::Contacts).expect("state");
    assert!(!contacts.allows_use(), "{contacts:?}");
}

pub(super) fn secure_storage_round_trips_in_the_keystore(_: &Instrumentation) {
    let store = AndroidSecureStorage;
    store.put("token", b"s3cret").expect("sealed");
    assert_eq!(store.get("token").expect("opened").as_deref(), Some(&b"s3cret"[..]));
    store.put("token", b"rotated").expect("replaced");
    assert_eq!(store.get("token").expect("opened").as_deref(), Some(&b"rotated"[..]));
    store.delete("token").expect("deleted");
    assert_eq!(store.get("token").expect("opened"), None);
    assert!(store.traits().hardware_backed, "this device keeps the key in secure hardware");
}

pub(super) fn http_honours_the_platform_policy_and_pins(_: &Instrumentation) {
    // The network security config's default: no cleartext to any host.
    let error = block_on(AndroidHttp::new().execute(HttpRequest::get("http://127.0.0.1:9/")))
        .expect_err("cleartext");
    assert!(error.to_string().to_lowercase().contains("cleartext"), "{error}");
    // A pinned host is never reached over plain HTTP.
    let pins = rustnative_core::CertificatePins::new().pin("example.com", [0; 32]);
    let pinned = AndroidHttp::new().with_pins(pins.clone());
    let error =
        block_on(pinned.execute(HttpRequest::get("http://example.com/"))).expect_err("pinned");
    assert!(error.to_string().contains("pinned"), "{error}");
    assert!(AndroidConditions.network(), "this suite runs on a device with a network");
    {
        let response =
            block_on(AndroidHttp::new().execute(HttpRequest::get("https://example.com/")))
                .expect("a response");
        assert_eq!(response.status(), 200);
        assert!(String::from_utf8_lossy(response.body_bytes()).contains("Example Domain"));
        let error = block_on(pinned.execute(HttpRequest::get("https://example.com/")))
            .expect_err("a wrong pin");
        assert!(error.to_string().contains("matches none of its pins"), "{error}");
    }
}

pub(super) fn icu_formats_collates_and_cases(_: &Instrumentation) {
    let german = Locale::new("de-DE");
    assert_eq!(AndroidLocale.format_number(&german, 1234.5, 2), "1.234,50");
    assert!(AndroidLocale.format_currency(&german, 3.5, "EUR").contains('€'));
    let date = AndroidLocale.format_date(
        &Locale::new("en-US"),
        Date { year: 2026, month: 9, day: 24 },
        DateStyle::Long,
    );
    assert_eq!(date, "September 24, 2026");
    // Swedish sorts ä after z; English beside a.
    assert_eq!(AndroidLocale.compare(&Locale::new("sv-SE"), "ä", "z"), std::cmp::Ordering::Greater);
    assert_eq!(AndroidLocale.compare(&Locale::new("en-US"), "ä", "z"), std::cmp::Ordering::Less);
    assert_eq!(AndroidLocale.to_upper(&Locale::new("tr-TR"), "i"), "İ");
}

fn bmp() -> Vec<u8> {
    // 4×2, 24-bit, all red.
    let mut out = Vec::new();
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54_u32 + 24).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54_u32.to_le_bytes());
    out.extend_from_slice(&40_u32.to_le_bytes());
    out.extend_from_slice(&4_i32.to_le_bytes());
    out.extend_from_slice(&2_i32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&24_u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for _ in 0..8 {
        out.extend_from_slice(&[0, 0, 255]);
    }
    out
}

pub(super) fn images_decode_and_downscale(_: &Instrumentation) {
    let image = BitmapDecoder.decode(&bmp(), Some((2, 2))).expect("decoded");
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(&image.pixels()[..4], &[255, 0, 0, 255], "RGBA red");
    let whole = BitmapDecoder.decode(&bmp(), None).expect("decoded");
    assert_eq!((whole.width(), whole.height()), (4, 2));
    assert!(BitmapDecoder.decode(b"not an image", None).is_err());
    let _ = AndroidConditions.charging();
}

pub(super) fn a_print_job_becomes_a_pdf(_: &Instrumentation) {
    let output = cache_dir().expect("a cache directory").join("suite.pdf");
    let job = PrintJob {
        title: "Suite".into(),
        pages: vec![vec!["Line one".into(), "Line two".into()], vec!["Page two".into()]],
        printer: None,
        output: Some(output.clone()),
    };
    AndroidPrinting.print(&job).expect("printed");
    let pdf = std::fs::read(&output).expect("the PDF");
    assert!(pdf.starts_with(b"%PDF"), "a PDF");
    assert_eq!(
        String::from_utf8_lossy(&pdf).matches("/Type /Page\n").count()
            + String::from_utf8_lossy(&pdf).matches("/Type /Page ").count(),
        2,
        "two pages"
    );
}

pub(super) fn state_persists_in_the_application_files(_: &Instrumentation) {
    let store = FileStateStore::for_application().expect("a store");
    assert!(store.directory().ends_with("rustnative/state"));
    store.save("suite", b"kept").expect("saved");
    let reopened = FileStateStore::for_application().expect("a store");
    assert_eq!(reopened.load("suite").expect("loaded").as_deref(), Some(&b"kept"[..]));
    reopened.remove("suite").expect("removed");
}

pub(super) fn push_and_billing_say_why_they_are_unavailable(_: &Instrumentation) {
    let push = block_on(AndroidPush.register(&["news".into()])).expect_err("no Firebase here");
    assert!(push.reason.contains("Firebase"), "{}", push.reason);
    let store = block_on(AndroidStore.products()).expect_err("no billing");
    assert!(store.reason.contains("Play Billing"), "{}", store.reason);
}

pub(super) fn a_dismissed_document_picker_answers_none(instrumentation: &Instrumentation) {
    let request = FileDialogRequest {
        kind: FileDialogKind::OpenFile,
        title: None,
        filters: vec![("Text".into(), vec!["txt".into()])],
        owner: None,
    };
    let picker = std::thread::spawn(move || block_on(AndroidFileDialogs.show(request)));
    // The picker is another application's activity: wait for it, then back out.
    let until = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let front = instrumentation.shell("dumpsys activity activities");
        let resumed = front
            .lines()
            .find(|line| line.contains("topResumedActivity") || line.contains("mResumedActivity"))
            .unwrap_or_default()
            .to_owned();
        if !resumed.is_empty() && !resumed.contains(&package()) {
            break;
        }
        assert!(std::time::Instant::now() < until, "the picker never came up: {resumed}");
        std::thread::sleep(Duration::from_millis(200));
    }
    std::thread::sleep(Duration::from_millis(500));
    let _ = call(&instrumentation.0, "sendKeyDownUpSync", "(I)V", &[Arg::Int(4)]);
    let chosen = picker.join().expect("the picker thread").expect("answered");
    assert_eq!(chosen, None, "backing out chooses nothing");
    instrumentation.bring_to_front();
}

fn package() -> String {
    java(Class::Services, "packageName", "()Ljava/lang/String;", &[]).string().unwrap_or_default()
}
