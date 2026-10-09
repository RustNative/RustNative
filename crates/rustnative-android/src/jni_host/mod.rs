//! The JNI boundary: the one module that names `jni::` (`PLAN.md`
//! Milestone 35: "JNI/FFI confined to one ownership module, with explicit
//! global-reference and thread-attachment rules").
//!
//! # Rules
//!
//! 1. **Ownership.** Every Java object Rust keeps beyond one call is a
//!    [`JavaRef`] — a JNI global reference owned by exactly one Rust value
//!    and deleted when it drops (on any thread: the `jni` crate attaches the
//!    dropping thread for the deletion if it must).
//! 2. **Local references** are created only inside [`with_env`], which runs
//!    its closure in a JNI local frame and pops it on return, so no call
//!    leaks locals into the frame of the Java callback that started the
//!    work — however many views one render touches.
//! 3. **Threads.** The main thread is a Java thread, attached for its life
//!    by the runtime. Any other thread that calls Java is attached as a
//!    daemon for its life on its first call ([`with_env`] does it), so a
//!    Tokio worker answering a service never pays an attach per call and
//!    never keeps the process from exiting.
//! 4. **Exceptions.** Every call that can throw is checked: a pending
//!    exception is cleared and turned into [`Error::Java`] carrying its
//!    class and message. Rust never returns to Java with an exception it
//!    caused still pending.
//! 5. **Classes.** The host library's classes are resolved once, in
//!    `JNI_OnLoad`, through the application's class loader (`FindClass`
//!    from a thread Rust attached would see only the system loader), and
//!    kept as global references.
//! 6. **Callbacks.** Every `native` entry runs its body under
//!    [`guard`], which catches a panic — unwinding into the JVM is
//!    undefined behaviour — and hands it to the backend's panic policy.

mod natives;

use std::sync::OnceLock;

use jni::JNIEnv;
use jni::objects::{
    GlobalRef, JByteArray, JClass, JFloatArray, JIntArray, JObject, JObjectArray, JString, JValue,
    JValueOwned,
};
use jni::sys::jsize;

use crate::Error;

pub(crate) use natives::on_load;

/// The process's Java VM, captured in `JNI_OnLoad`.
static VM: OnceLock<jni::JavaVM> = OnceLock::new();
/// The host library's classes, resolved in `JNI_OnLoad`.
static CLASSES: OnceLock<Vec<Option<GlobalRef>>> = OnceLock::new();

/// A global reference to the object `raw` (a `jobject`, local or global)
/// refers to.
///
/// # Safety
///
/// `raw` is a live JNI reference valid on this thread.
pub(crate) unsafe fn adopt_raw(raw: *mut std::ffi::c_void) -> Option<JavaRef> {
    if raw.is_null() {
        return None;
    }
    with_env(|env| {
        // SAFETY: the caller's contract; the borrowed object is only read to
        // make a global reference.
        // A `JObject` owns nothing, so dropping it leaves the reference alone.
        let object = unsafe { JObject::from_raw(raw.cast()) };
        global(env, &object)
    })
    .ok()
}

/// The VM as a raw `JavaVM*` (null before `JNI_OnLoad`).
pub(crate) fn raw_vm() -> *mut std::ffi::c_void {
    VM.get().map_or(std::ptr::null_mut(), |vm| vm.get_java_vm_pointer().cast())
}

/// A class of the host library (or the platform) Rust calls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Class {
    Bridge,
    Views,
    Layout,
    Style,
    Measure,
    Activity,
    Menus,
    Instrumentation,
    Services,
    Access,
    Canvas,
    Surface,
    Platform,
    Intents,
    Posture,
    Probe,
    Host,
    Input,
    Jobs,
}

impl Class {
    pub(crate) const ALL: [Self; 19] = [
        Self::Bridge,
        Self::Views,
        Self::Layout,
        Self::Style,
        Self::Measure,
        Self::Activity,
        Self::Menus,
        Self::Instrumentation,
        Self::Services,
        Self::Access,
        Self::Canvas,
        Self::Surface,
        Self::Platform,
        Self::Intents,
        Self::Posture,
        Self::Probe,
        Self::Host,
        Self::Input,
        Self::Jobs,
    ];

    /// The class's JNI name.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Bridge => "dev/rustnative/android/RnBridge",
            Self::Views => "dev/rustnative/android/RnViews",
            Self::Layout => "dev/rustnative/android/RnLayout",
            Self::Style => "dev/rustnative/android/RnStyle",
            Self::Measure => "dev/rustnative/android/RnMeasure",
            Self::Activity => "dev/rustnative/android/RnActivity",
            Self::Menus => "dev/rustnative/android/RnMenus",
            Self::Instrumentation => "dev/rustnative/android/RnInstrumentation",
            Self::Services => "dev/rustnative/android/RnServices",
            Self::Access => "dev/rustnative/android/RnAccess",
            Self::Canvas => "dev/rustnative/android/RnCanvasView",
            Self::Surface => "dev/rustnative/android/RnSurfaceView",
            Self::Platform => "dev/rustnative/android/RnPlatform",
            Self::Intents => "dev/rustnative/android/RnIntents",
            Self::Posture => "dev/rustnative/android/RnPosture",
            Self::Probe => "dev/rustnative/android/RnProbe",
            Self::Host => "dev/rustnative/android/RnHost",
            Self::Input => "dev/rustnative/android/RnInput",
            Self::Jobs => "dev/rustnative/android/RnJobService",
        }
    }
}

/// A Java object Rust holds: a global reference, deleted on drop.
#[derive(Clone)]
pub(crate) struct JavaRef(GlobalRef);

impl JavaRef {
    pub(crate) fn as_obj(&self) -> &JObject<'static> {
        self.0.as_obj()
    }
}

impl std::fmt::Debug for JavaRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JavaRef(..)")
    }
}

/// An argument to a Java method.
#[derive(Debug)]
pub(crate) enum Arg<'a> {
    Obj(&'a JavaRef),
    Null,
    Bool(bool),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Str(&'a str),
    OptStr(Option<&'a str>),
    Ints(&'a [i32]),
    Floats(&'a [f32]),
    Bytes(&'a [u8]),
    Strs(&'a [String]),
}

/// What a Java method returned.
#[derive(Debug)]
pub(crate) enum Ret {
    Void,
    Bool(bool),
    #[cfg_attr(
        not(feature = "device-tests"),
        allow(dead_code, reason = "read by the device suite")
    )]
    Int(i32),
    Long(i64),
    Float(f32),
    Obj(Option<JavaRef>),
    Str(Option<String>),
    Ints(Vec<i32>),
    Floats(Vec<f32>),
    Strs(Vec<String>),
    /// A `byte[]`; `None` for a null array.
    Bytes(Option<Vec<u8>>),
}

impl Ret {
    pub(crate) fn bool(self) -> bool {
        matches!(self, Self::Bool(true))
    }
    #[cfg(feature = "device-tests")]
    pub(crate) fn int(self) -> i32 {
        if let Self::Int(value) = self { value } else { 0 }
    }
    pub(crate) fn long(self) -> i64 {
        if let Self::Long(value) = self { value } else { 0 }
    }
    pub(crate) fn float(self) -> f32 {
        if let Self::Float(value) = self { value } else { 0.0 }
    }
    pub(crate) fn obj(self) -> Option<JavaRef> {
        if let Self::Obj(value) = self { value } else { None }
    }
    pub(crate) fn string(self) -> Option<String> {
        if let Self::Str(value) = self { value } else { None }
    }
    pub(crate) fn ints(self) -> Vec<i32> {
        if let Self::Ints(value) = self { value } else { Vec::new() }
    }
    pub(crate) fn floats(self) -> Vec<f32> {
        if let Self::Floats(value) = self { value } else { Vec::new() }
    }
    pub(crate) fn strings(self) -> Vec<String> {
        if let Self::Strs(value) = self { value } else { Vec::new() }
    }
}

/// Runs `f` with this thread's `JNIEnv`, inside a local frame (rule 2),
/// attaching the thread as a daemon first if it is not attached (rule 3).
pub(crate) fn with_env<R>(f: impl FnOnce(&mut JNIEnv<'_>) -> Result<R, Error>) -> Result<R, Error> {
    let vm = VM.get().ok_or(Error::UnsupportedHost)?;
    let mut env = match vm.get_env() {
        Ok(env) => env,
        Err(_) => vm
            .attach_current_thread_as_daemon()
            .map_err(|error| jni_error("AttachCurrentThread", &error))?,
    };
    let mut outcome = None;
    let framed = env.with_local_frame(64, |env| -> Result<(), jni::errors::Error> {
        outcome = Some(f(env));
        Ok(())
    });
    if let Err(error) = framed {
        return Err(jni_error("PushLocalFrame", &error));
    }
    outcome.unwrap_or_else(|| Err(Error::java("PushLocalFrame", "the frame did not run")))
}

fn class(class: Class) -> Result<&'static GlobalRef, Error> {
    CLASSES
        .get()
        .and_then(|classes| classes.get(class as usize))
        .and_then(Option::as_ref)
        .ok_or_else(|| {
            Error::java(
                class.name(),
                "the class is not in this application (or the library is not loaded)",
            )
        })
}

/// Calls static method `name` with signature `signature` on `class`.
pub(crate) fn call_static(
    class_id: Class,
    name: &str,
    signature: &str,
    args: &[Arg<'_>],
) -> Result<Ret, Error> {
    let class_ref = class(class_id)?;
    with_env(|env| {
        let values = to_values(env, args)?;
        let borrowed: Vec<JValue<'_, '_>> = values.iter().map(JValueOwned::borrow).collect();
        let jclass: &JClass<'_> = <&JClass<'_>>::from(class_ref.as_obj());
        let result = env.call_static_method(jclass, name, signature, &borrowed);
        let value = checked(env, result, || format!("{}.{name}", short(class_id.name())))?;
        from_value(env, value, signature)
    })
}

/// Calls instance method `name` with signature `signature` on `object`.
#[cfg_attr(
    not(feature = "device-tests"),
    allow(dead_code, reason = "the backend calls statics; the device suite calls instances")
)]
pub(crate) fn call(
    object: &JavaRef,
    name: &str,
    signature: &str,
    args: &[Arg<'_>],
) -> Result<Ret, Error> {
    with_env(|env| {
        let values = to_values(env, args)?;
        let borrowed: Vec<JValue<'_, '_>> = values.iter().map(JValueOwned::borrow).collect();
        let result = env.call_method(object.as_obj(), name, signature, &borrowed);
        let value = checked(env, result, || name.to_owned())?;
        from_value(env, value, signature)
    })
}

/// Sets static field `name` (a `boolean`) of `class`.
pub(crate) fn set_static_bool(class_id: Class, name: &str, value: bool) -> Result<(), Error> {
    let class_ref = class(class_id)?;
    with_env(|env| {
        let jclass: &JClass<'_> = <&JClass<'_>>::from(class_ref.as_obj());
        let result =
            env.set_static_field(jclass, (jclass, name, "Z"), JValue::Bool(u8::from(value)));
        checked(env, result, || format!("{}.{name}", short(class_id.name())))
    })
}

/// Turns a JNI result into ours, clearing and describing a pending
/// exception (rule 4).
fn checked<T>(
    env: &mut JNIEnv<'_>,
    result: jni::errors::Result<T>,
    operation: impl FnOnce() -> String,
) -> Result<T, Error> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let detail = pending_exception(env).unwrap_or_else(|| error.to_string());
            Err(Error::java(operation(), detail))
        }
    }
}

/// Clears a pending exception and returns its description.
pub(crate) fn pending_exception(env: &mut JNIEnv<'_>) -> Option<String> {
    if !env.exception_check().unwrap_or(false) {
        return None;
    }
    let throwable = env.exception_occurred().ok();
    let _ = env.exception_clear();
    let throwable = throwable?;
    let text = env
        .call_method(&throwable, "toString", "()Ljava/lang/String;", &[])
        .ok()
        .and_then(|value| value.l().ok())
        .and_then(|object| read_string(env, &JString::from(object)));
    let _ = env.exception_clear();
    Some(text.unwrap_or_else(|| "a Java exception".to_owned()))
}

fn jni_error(operation: &str, error: &jni::errors::Error) -> Error {
    Error::java(operation, error.to_string())
}

fn short(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn to_values<'local>(
    env: &mut JNIEnv<'local>,
    args: &[Arg<'_>],
) -> Result<Vec<JValueOwned<'local>>, Error> {
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        let value = match arg {
            Arg::Obj(object) => JValueOwned::Object(
                env.new_local_ref(object.as_obj()).map_err(|e| jni_error("NewLocalRef", &e))?,
            ),
            Arg::Bool(value) => JValueOwned::Bool(u8::from(*value)),
            Arg::Int(value) => JValueOwned::Int(*value),
            Arg::Long(value) => JValueOwned::Long(*value),
            Arg::Float(value) => JValueOwned::Float(*value),
            Arg::Double(value) => JValueOwned::Double(*value),
            Arg::Str(text) => JValueOwned::Object(
                env.new_string(text).map_err(|e| jni_error("NewStringUTF", &e))?.into(),
            ),
            Arg::Null | Arg::OptStr(None) => JValueOwned::Object(JObject::null()),
            Arg::OptStr(Some(text)) => JValueOwned::Object(
                env.new_string(text).map_err(|e| jni_error("NewStringUTF", &e))?.into(),
            ),
            Arg::Ints(ints) => {
                let array = env
                    .new_int_array(jsize::try_from(ints.len()).unwrap_or(jsize::MAX))
                    .map_err(|e| jni_error("NewIntArray", &e))?;
                env.set_int_array_region(&array, 0, ints)
                    .map_err(|e| jni_error("SetIntArrayRegion", &e))?;
                JValueOwned::Object(array.into())
            }
            Arg::Floats(floats) => {
                let array = env
                    .new_float_array(jsize::try_from(floats.len()).unwrap_or(jsize::MAX))
                    .map_err(|e| jni_error("NewFloatArray", &e))?;
                env.set_float_array_region(&array, 0, floats)
                    .map_err(|e| jni_error("SetFloatArrayRegion", &e))?;
                JValueOwned::Object(array.into())
            }
            Arg::Bytes(bytes) => JValueOwned::Object(
                env.byte_array_from_slice(bytes).map_err(|e| jni_error("NewByteArray", &e))?.into(),
            ),
            Arg::Strs(strings) => {
                let array = env
                    .new_object_array(
                        jsize::try_from(strings.len()).unwrap_or(jsize::MAX),
                        "java/lang/String",
                        JObject::null(),
                    )
                    .map_err(|e| jni_error("NewObjectArray", &e))?;
                for (index, text) in strings.iter().enumerate() {
                    let string = env.new_string(text).map_err(|e| jni_error("NewStringUTF", &e))?;
                    env.set_object_array_element(
                        &array,
                        jsize::try_from(index).unwrap_or(jsize::MAX),
                        string,
                    )
                    .map_err(|e| jni_error("SetObjectArrayElement", &e))?;
                }
                JValueOwned::Object(array.into())
            }
        };
        values.push(value);
    }
    Ok(values)
}

/// The return type of a method signature: what follows `)`.
fn return_type(signature: &str) -> &str {
    signature.rsplit_once(')').map_or("V", |(_, ret)| ret)
}

fn from_value(env: &mut JNIEnv<'_>, value: JValueOwned<'_>, signature: &str) -> Result<Ret, Error> {
    let ret = return_type(signature);
    Ok(match ret {
        "V" => Ret::Void,
        "Z" => Ret::Bool(value.z().unwrap_or(false)),
        "I" => Ret::Int(value.i().unwrap_or(0)),
        "J" => Ret::Long(value.j().unwrap_or(0)),
        "F" => Ret::Float(value.f().unwrap_or(0.0)),
        "Ljava/lang/String;" => {
            let object = value.l().map_err(|e| jni_error("String result", &e))?;
            Ret::Str(read_string(env, &JString::from(object)))
        }
        "[I" => {
            let object = value.l().map_err(|e| jni_error("int[] result", &e))?;
            Ret::Ints(read_ints(env, &JIntArray::from(object)))
        }
        "[F" => {
            let object = value.l().map_err(|e| jni_error("float[] result", &e))?;
            Ret::Floats(read_floats(env, &JFloatArray::from(object)))
        }
        "[B" => {
            let object = value.l().map_err(|e| jni_error("byte[] result", &e))?;
            let array = JByteArray::from(object);
            Ret::Bytes((!array.is_null()).then(|| read_bytes(env, &array)))
        }
        "[Ljava/lang/String;" => {
            let object = value.l().map_err(|e| jni_error("String[] result", &e))?;
            Ret::Strs(read_strings(env, &JObjectArray::from(object)))
        }
        _ => {
            let object = value.l().map_err(|e| jni_error("object result", &e))?;
            if object.is_null() { Ret::Obj(None) } else { Ret::Obj(Some(global(env, &object)?)) }
        }
    })
}

/// A global reference to `object` (rule 1).
pub(crate) fn global(env: &mut JNIEnv<'_>, object: &JObject<'_>) -> Result<JavaRef, Error> {
    env.new_global_ref(object).map(JavaRef).map_err(|e| jni_error("NewGlobalRef", &e))
}

pub(crate) fn read_string(env: &mut JNIEnv<'_>, string: &JString<'_>) -> Option<String> {
    if string.is_null() {
        return None;
    }
    env.get_string(string).ok().map(String::from)
}

pub(crate) fn read_ints(env: &mut JNIEnv<'_>, array: &JIntArray<'_>) -> Vec<i32> {
    if array.is_null() {
        return Vec::new();
    }
    let length = env.get_array_length(array).unwrap_or(0);
    let mut out = vec![0; usize::try_from(length).unwrap_or(0)];
    if env.get_int_array_region(array, 0, &mut out).is_err() {
        let _ = pending_exception(env);
        return Vec::new();
    }
    out
}

pub(crate) fn read_floats(env: &mut JNIEnv<'_>, array: &JFloatArray<'_>) -> Vec<f32> {
    if array.is_null() {
        return Vec::new();
    }
    let length = env.get_array_length(array).unwrap_or(0);
    let mut out = vec![0.0; usize::try_from(length).unwrap_or(0)];
    if env.get_float_array_region(array, 0, &mut out).is_err() {
        let _ = pending_exception(env);
        return Vec::new();
    }
    out
}

pub(crate) fn read_bytes(env: &mut JNIEnv<'_>, array: &JByteArray<'_>) -> Vec<u8> {
    if array.is_null() {
        return Vec::new();
    }
    env.convert_byte_array(array).unwrap_or_default()
}

pub(crate) fn read_strings(env: &mut JNIEnv<'_>, array: &JObjectArray<'_>) -> Vec<String> {
    if array.is_null() {
        return Vec::new();
    }
    let length = env.get_array_length(array).unwrap_or(0);
    let mut out = Vec::with_capacity(usize::try_from(length).unwrap_or(0));
    for index in 0..length {
        let element = env.get_object_array_element(array, index).ok();
        let text = element.and_then(|element| read_string(env, &JString::from(element)));
        out.push(text.unwrap_or_default());
    }
    out
}

#[link(name = "android")]
unsafe extern "C" {
    fn ANativeWindow_fromSurface(
        env: *mut jni::sys::JNIEnv,
        surface: jni::sys::jobject,
    ) -> *mut std::ffi::c_void;
    fn ANativeWindow_release(window: *mut std::ffi::c_void);
}

/// The `ANativeWindow` of a `android.view.Surface`, acquired (released
/// with [`release_native_window`]).
pub(crate) fn native_window(surface: &JavaRef) -> Result<*mut std::ffi::c_void, Error> {
    with_env(|env| {
        // SAFETY: `env` is this thread's live JNIEnv and `surface` a live
        // global reference to an `android.view.Surface`; the window
        // returned is acquired for the caller.
        let window = unsafe { ANativeWindow_fromSurface(env.get_raw(), surface.as_obj().as_raw()) };
        if window.is_null() {
            Err(Error::java("ANativeWindow_fromSurface", "the surface has no window"))
        } else {
            Ok(window)
        }
    })
}

/// Releases a window [`native_window`] acquired.
pub(crate) fn release_native_window(window: *mut std::ffi::c_void) {
    if !window.is_null() {
        // SAFETY: `window` was acquired by `native_window` and is released
        // exactly once (the surface registry forgets it first).
        unsafe { ANativeWindow_release(window) };
    }
}

/// Runs a native entry's body, catching a panic (rule 6).
pub(crate) fn guard<R: Default>(entry: &str, body: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(value) => value,
        Err(payload) => {
            let message = crate::backend::panic_message(payload.as_ref());
            crate::log::error(&format!("a panic reached the JNI boundary in {entry}: {message}"));
            crate::backend::panicked(message);
            R::default()
        }
    }
}
