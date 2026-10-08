//! `JNI_OnLoad` and the `native` methods of `RnBridge`: where Java calls
//! Rust. Each entry converts its arguments out of JNI types and hands them
//! to the backend under [`super::guard`].

use std::ffi::c_void;

use jni::JNIEnv;
use jni::NativeMethod;
use jni::objects::{JByteArray, JClass, JFloatArray, JIntArray, JObject, JObjectArray, JString};
use jni::sys::{JNI_FALSE, JNI_TRUE, JNI_VERSION_1_6, jboolean, jfloat, jint, jlong};

use super::{
    Class, JavaRef, global, read_bytes, read_floats, read_ints, read_string, read_strings,
};

/// What `JNI_OnLoad` does (`export_main!` expands to a `JNI_OnLoad` that
/// calls this): captures the VM, resolves the host library's classes, and
/// registers `RnBridge`'s natives. `main` is the application's portable
/// `main`, run when the launcher activity is created; `None` for a library
/// linked into an existing application (Milestone 40's library-only mode).
///
/// # Safety
///
/// `vm` is the pointer the runtime passed to `JNI_OnLoad`.
pub(crate) unsafe fn on_load(vm: *mut jni::sys::JavaVM, main: Option<fn()>) -> jint {
    // SAFETY: the runtime's own VM pointer, valid for the process (the
    // caller's contract).
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(vm) }) else { return -1 };
    let _ = super::VM.set(vm);
    crate::entry::set_main(main);
    let Some(vm) = super::VM.get() else { return -1 };
    let Ok(mut env) = vm.get_env() else { return -1 };
    let mut classes = Vec::with_capacity(Class::ALL.len());
    for class in Class::ALL {
        // A class an application left out (its services, say) is answered
        // when it is called, not here.
        let found =
            env.find_class(class.name()).ok().and_then(|found| env.new_global_ref(found).ok());
        let _ = super::pending_exception(&mut env);
        classes.push(found);
    }
    let _ = super::CLASSES.set(classes);
    let Ok(bridge) = env.find_class(Class::Bridge.name()) else {
        let _ = super::pending_exception(&mut env);
        crate::log::error("RnBridge is missing: the host library was not packaged");
        return -1;
    };
    if env.register_native_methods(&bridge, &methods()).is_err() {
        let detail = super::pending_exception(&mut env).unwrap_or_default();
        crate::log::error(&format!("registering RnBridge's natives failed: {detail}"));
        return -1;
    }
    JNI_VERSION_1_6
}

fn method(name: &str, signature: &str, function: *mut c_void) -> NativeMethod {
    NativeMethod { name: name.into(), sig: signature.into(), fn_ptr: function }
}

fn methods() -> Vec<NativeMethod> {
    vec![
        method("nativeInit", "(Landroid/content/Context;)V", native_init as *mut c_void),
        method(
            "nativeCreate",
            "(Landroid/app/Activity;JLdev/rustnative/android/RnLayout;ZLandroid/content/Intent;)V",
            native_create as *mut c_void,
        ),
        method("nativeLifecycle", "(JII)V", native_lifecycle as *mut c_void),
        method("nativeResized", "(JII)V", native_resized as *mut c_void),
        method("nativeInsets", "(J[I)V", native_insets as *mut c_void),
        method("nativeViewEvent", "(JIIJJLjava/lang/String;)V", native_view_event as *mut c_void),
        method("nativeKey", "(JIIIIII)Z", native_key as *mut c_void),
        method("nativePointer", "(JII[I[I[FIIJ)Z", native_pointer as *mut c_void),
        method("nativeBack", "(JIFI)V", native_back as *mut c_void),
        method("nativeGamepad", "(JIII[F)Z", native_gamepad as *mut c_void),
        method("nativeText", "(JIILjava/lang/String;I)V", native_text as *mut c_void),
        method("nativeIntent", "(JLandroid/content/Intent;)V", native_intent as *mut c_void),
        method(
            "nativeActivityResult",
            "(IILandroid/content/Intent;)V",
            native_activity_result as *mut c_void,
        ),
        method("nativePermissions", "(I[Ljava/lang/String;[I)V", native_permissions as *mut c_void),
        method("nativeIdle", "()V", native_idle as *mut c_void),
        method("nativeFrame", "(JJ)V", native_frame as *mut c_void),
        method("nativeTimer", "(J)V", native_timer as *mut c_void),
        method(
            "nativeServiceReply",
            "(JILjava/lang/String;[B)V",
            native_service_reply as *mut c_void,
        ),
        method("nativeRunJob", "(Ljava/lang/String;)Z", native_run_job as *mut c_void),
        method(
            "nativeRunTests",
            "(Landroid/app/Instrumentation;Ljava/lang/String;)V",
            native_run_tests as *mut c_void,
        ),
    ]
}

fn reference(env: &mut JNIEnv<'_>, object: &JObject<'_>) -> Option<JavaRef> {
    if object.is_null() { None } else { global(env, object).ok() }
}

/// A window id from Java's `long` (window ids are small and positive).
fn window(raw: jlong) -> u64 {
    u64::try_from(raw).unwrap_or(0)
}

extern "system" fn native_init(mut env: JNIEnv<'_>, _class: JClass<'_>, context: JObject<'_>) {
    super::guard("nativeInit", || {
        if let Some(context) = reference(&mut env, &context) {
            crate::entry::init(context);
        }
    });
}

extern "system" fn native_create(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    activity: JObject<'_>,
    window_id: jlong,
    root: JObject<'_>,
    restored: jboolean,
    intent: JObject<'_>,
) {
    super::guard("nativeCreate", || {
        let (Some(activity), Some(root)) =
            (reference(&mut env, &activity), reference(&mut env, &root))
        else {
            return;
        };
        let intent = reference(&mut env, &intent);
        crate::entry::activity_created(
            activity,
            window(window_id),
            root,
            restored != JNI_FALSE,
            intent,
        );
    });
}

extern "system" fn native_lifecycle(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    what: jint,
    argument: jint,
) {
    super::guard("nativeLifecycle", || {
        crate::backend::lifecycle(window(window_id), what, argument);
    });
}

extern "system" fn native_resized(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    width: jint,
    height: jint,
) {
    super::guard("nativeResized", || crate::backend::resized(window(window_id), width, height));
}

extern "system" fn native_insets(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    insets: JIntArray<'_>,
) {
    super::guard("nativeInsets", || {
        let values = read_ints(&mut env, &insets);
        let mut insets = [0; 16];
        for (slot, value) in insets.iter_mut().zip(values) {
            *slot = value;
        }
        crate::backend::insets(window(window_id), insets);
    });
}

extern "system" fn native_view_event(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    tag: jint,
    event: jint,
    a: jlong,
    b: jlong,
    text: JString<'_>,
) {
    super::guard("nativeViewEvent", || {
        let text = read_string(&mut env, &text);
        crate::backend::view_event(window(window_id), tag, event, a, b, text);
    });
}

#[allow(clippy::too_many_arguments, reason = "a JNI entry's arguments are the Java method's")]
extern "system" fn native_key(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    action: jint,
    key_code: jint,
    meta: jint,
    unicode: jint,
    repeat: jint,
    source: jint,
) -> jboolean {
    let taken = super::guard("nativeKey", || {
        crate::backend::key(
            window(window_id),
            crate::input::KeyFrame { action, key_code, meta, unicode, repeat, source },
        )
    });
    if taken { JNI_TRUE } else { JNI_FALSE }
}

#[allow(clippy::too_many_arguments, reason = "a JNI entry's arguments are the Java method's")]
extern "system" fn native_pointer(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    action: jint,
    action_index: jint,
    ids: JIntArray<'_>,
    tools: JIntArray<'_>,
    values: JFloatArray<'_>,
    buttons: jint,
    meta: jint,
    time: jlong,
) -> jboolean {
    let claimed = super::guard("nativePointer", || {
        let frame = crate::input::PointerFrame {
            action,
            action_index,
            ids: read_ints(&mut env, &ids),
            tools: read_ints(&mut env, &tools),
            values: read_floats(&mut env, &values),
            buttons,
            meta,
            time,
        };
        crate::backend::pointer(window(window_id), &frame)
    });
    if claimed { JNI_TRUE } else { JNI_FALSE }
}

extern "system" fn native_gamepad(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    device: jint,
    key_code: jint,
    action: jint,
    axes: JFloatArray<'_>,
) -> jboolean {
    let taken = super::guard("nativeGamepad", || {
        let axes = (!axes.is_null()).then(|| read_floats(&mut env, &axes));
        crate::backend::gamepad(window(window_id), device, key_code, action, axes.as_deref())
    });
    if taken { JNI_TRUE } else { JNI_FALSE }
}

extern "system" fn native_text(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    tag: jint,
    step: jint,
    text: JString<'_>,
    cursor: jint,
) {
    super::guard("nativeText", || {
        let text = read_string(&mut env, &text).unwrap_or_default();
        crate::backend::text(window(window_id), tag, step, text, cursor);
    });
}

extern "system" fn native_back(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    phase: jint,
    progress: jfloat,
    edge: jint,
) {
    super::guard("nativeBack", || crate::backend::back(window(window_id), phase, progress, edge));
}

extern "system" fn native_intent(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    intent: JObject<'_>,
) {
    super::guard("nativeIntent", || {
        if let Some(intent) = reference(&mut env, &intent) {
            crate::backend::intent(window(window_id), &intent);
        }
    });
}

extern "system" fn native_activity_result(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    request: jint,
    result: jint,
    data: JObject<'_>,
) {
    super::guard("nativeActivityResult", || {
        let data = reference(&mut env, &data);
        crate::services::activity_result(request, result, data);
    });
}

extern "system" fn native_permissions(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    request: jint,
    permissions: JObjectArray<'_>,
    results: JIntArray<'_>,
) {
    super::guard("nativePermissions", || {
        let permissions = read_strings(&mut env, &permissions);
        let results = read_ints(&mut env, &results);
        crate::services::permissions_answered(request, &permissions, &results);
    });
}

extern "system" fn native_idle(_env: JNIEnv<'_>, _class: JClass<'_>) {
    super::guard("nativeIdle", crate::backend::idle);
}

extern "system" fn native_frame(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    window_id: jlong,
    nanos: jlong,
) {
    super::guard("nativeFrame", || crate::backend::frame(window(window_id), nanos));
}

extern "system" fn native_timer(_env: JNIEnv<'_>, _class: JClass<'_>, token: jlong) {
    super::guard("nativeTimer", || crate::backend::timer(token));
}

extern "system" fn native_service_reply(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    token: jlong,
    status: jint,
    text: JString<'_>,
    bytes: JByteArray<'_>,
) {
    super::guard("nativeServiceReply", || {
        let text = read_string(&mut env, &text);
        let bytes = read_bytes(&mut env, &bytes);
        crate::services::reply(token, status, text, bytes);
    });
}

extern "system" fn native_run_job(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    name: JString<'_>,
) -> jboolean {
    let again = super::guard("nativeRunJob", || {
        let name = read_string(&mut env, &name).unwrap_or_default();
        crate::services::run_job(&name)
    });
    if again { JNI_TRUE } else { JNI_FALSE }
}

extern "system" fn native_run_tests(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    instrumentation: JObject<'_>,
    filter: JString<'_>,
) {
    super::guard("nativeRunTests", || {
        let filter = read_string(&mut env, &filter).unwrap_or_default();
        let Some(instrumentation) = reference(&mut env, &instrumentation) else { return };
        #[cfg(feature = "device-tests")]
        crate::device_tests::run(&instrumentation, &filter);
        #[cfg(not(feature = "device-tests"))]
        {
            let _ = (instrumentation, filter);
            crate::log::error("this build has no device suite (the `device-tests` feature is off)");
        }
    });
}
