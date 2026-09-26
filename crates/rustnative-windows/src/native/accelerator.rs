//! Accelerator availability (`PLAN.md` Milestone 51, `C89-1`), answered
//! from the machine rather than assumed: a GPU when DXGI lists a hardware
//! adapter, an NPU when `DXCore` lists a machine-learning adapter that is
//! not also a graphics adapter.

use windows::Win32::Graphics::DXCore::{
    DXCORE_ADAPTER_ATTRIBUTE_D3D12_GRAPHICS, IDXCoreAdapter, IDXCoreAdapterFactory,
    IDXCoreAdapterList,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};
use windows_core::{GUID, HRESULT, Interface};

/// Whether a hardware graphics adapter is present.
pub(crate) fn gpu_available() -> bool {
    // SAFETY: plain factory creation; every returned interface is owned.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return false;
    };
    let software = u32::try_from(DXGI_ADAPTER_FLAG_SOFTWARE.0).unwrap_or(0);
    (0..)
        // SAFETY: enumeration stops at the first index with no adapter.
        .map_while(|index| unsafe { factory.EnumAdapters1(index) }.ok())
        // SAFETY: a live adapter's description.
        .filter_map(|adapter| unsafe { adapter.GetDesc1() }.ok())
        .any(|description| description.Flags & software == 0)
}

/// `DXCORE_ADAPTER_ATTRIBUTE_D3D12_GENERIC_ML` from `dxcore_interface.h`,
/// which this version of the bindings does not carry.
const GENERIC_ML: GUID = GUID::from_u128(0xb71b_0d41_1088_422f_a27c_0250_b7d3_a988);

type CreateFactory = unsafe extern "system" fn(*const GUID, *mut *mut core::ffi::c_void) -> HRESULT;

/// Whether a neural processing unit is present. `DXCore` is loaded at run
/// time: it ships with Windows 10 2004 and later, and an older system
/// simply has no NPU to report.
pub(crate) fn npu_available() -> bool {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let name = crate::native::util::wide("dxcore.dll");
    // SAFETY: a NUL-terminated library name.
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return false;
    }
    // SAFETY: a NUL-terminated export name in a loaded module.
    let Some(create) =
        (unsafe { GetProcAddress(module, c"DXCoreCreateAdapterFactory".as_ptr().cast()) })
    else {
        return false;
    };
    // SAFETY: the export has this documented signature.
    let create: CreateFactory = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateFactory>(create)
    };
    let mut raw = std::ptr::null_mut();
    // SAFETY: an IID and an out-pointer, as the function takes.
    if unsafe { create(&IDXCoreAdapterFactory::IID, &raw mut raw) }.is_err() || raw.is_null() {
        return false;
    }
    // SAFETY: a factory the call above returned, owned from here.
    let factory = unsafe { IDXCoreAdapterFactory::from_raw(raw) };
    // SAFETY: a live factory and a valid attribute list.
    let Ok(list) = (unsafe { factory.CreateAdapterList::<IDXCoreAdapterList>(&[GENERIC_ML]) })
    else {
        return false;
    };
    // SAFETY: a live list.
    let count = unsafe { list.GetAdapterCount() };
    (0..count)
        // SAFETY: indices below the list's count.
        .filter_map(|index| unsafe { list.GetAdapter::<IDXCoreAdapter>(index) }.ok())
        // SAFETY: a live adapter and a valid attribute.
        .any(|adapter| !unsafe {
            adapter.IsAttributeSupported(&DXCORE_ADAPTER_ATTRIBUTE_D3D12_GRAPHICS)
        })
}
