//! Manual COM binding for the non-public IPolicyConfig interface.
//!
//! IPolicyConfig::SetDefaultEndpoint is the only known way to change the
//! system-wide default audio endpoint programmatically. There is no public
//! header for it. GUIDs and vtable layout come from long-standing reverse
//! engineering (documented in EarTrumpet, SoundVolumeView, and many blog
//! posts since Windows Vista).
//!
//! We try two IIDs in order: the Win10/11 shape first, the older Vista shape
//! as a fallback. Both expose `SetDefaultEndpoint` at vtable slot 13 with the
//! same signature.

use std::ffi::c_void;
use std::ptr;

use windows::core::{IUnknown, Interface, GUID, HRESULT, PCWSTR};
use windows::Win32::Media::Audio::{eCommunications, eConsole, eMultimedia, ERole};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

// CLSID_CPolicyConfigClient
const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

// IID_IPolicyConfig (Windows 10 / 11)
const IID_IPOLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);

// IID_IPolicyConfigVista (older shape, kept for defensive fallback)
const IID_IPOLICY_CONFIG_VISTA: GUID = GUID::from_u128(0x568b9108_44bf_40b4_9006_86afe5b5a620);

/// Vtable layout common to both IPolicyConfig and IPolicyConfigVista at the
/// slots we care about. The methods before SetDefaultEndpoint are opaque —
/// we only need their slots reserved, not typed.
#[repr(C)]
struct IPolicyConfigVtbl {
    // IUnknown
    query_interface: unsafe extern "system" fn(
        this: *mut c_void,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT,
    add_ref: unsafe extern "system" fn(this: *mut c_void) -> u32,
    release: unsafe extern "system" fn(this: *mut c_void) -> u32,

    // Slots we do not use — leave as opaque pointers so the layout matches.
    _get_mix_format: *const c_void,        // 3
    _get_device_format: *const c_void,     // 4
    _reset_device_format: *const c_void,   // 5
    _set_device_format: *const c_void,     // 6
    _get_processing_period: *const c_void, // 7
    _set_processing_period: *const c_void, // 8
    _get_share_mode: *const c_void,        // 9
    _set_share_mode: *const c_void,        // 10
    _get_property_value: *const c_void,    // 11
    _set_property_value: *const c_void,    // 12

    // Slot 13 — the one we need.
    set_default_endpoint:
        unsafe extern "system" fn(this: *mut c_void, device_id: PCWSTR, role: ERole) -> HRESULT,

    _set_endpoint_visibility: *const c_void, // 14
}

/// Thin RAII wrapper: holds the raw pointer + vtable and calls Release on drop.
struct PolicyConfig {
    raw: *mut c_void,
    vtbl: *const IPolicyConfigVtbl,
}

impl PolicyConfig {
    /// Try Win10/11 IID first, then Vista IID. Returns a wrapper or a
    /// windows::core::Error from the last failing attempt.
    fn create() -> windows::core::Result<Self> {
        let unknown: IUnknown =
            unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }?;

        for iid in [IID_IPOLICY_CONFIG, IID_IPOLICY_CONFIG_VISTA] {
            let mut raw: *mut c_void = ptr::null_mut();
            let hr = unsafe {
                (Interface::vtable(&unknown).QueryInterface)(unknown.as_raw(), &iid, &mut raw)
            };
            if hr.is_ok() && !raw.is_null() {
                // Vtable pointer is the first field of a COM object.
                let vtbl = unsafe { *(raw as *const *const IPolicyConfigVtbl) };
                return Ok(PolicyConfig { raw, vtbl });
            }
        }
        // Fall through: return the last-attempt error.
        Err(windows::core::Error::from_hresult(HRESULT(
            0x80004002u32 as i32,
        ))) // E_NOINTERFACE
    }

    fn set_default_endpoint(&self, device_id: &str, role: ERole) -> windows::core::Result<()> {
        let wide: Vec<u16> = device_id.encode_utf16().chain(std::iter::once(0)).collect();
        let hr =
            unsafe { ((*self.vtbl).set_default_endpoint)(self.raw, PCWSTR(wide.as_ptr()), role) };
        if hr.is_ok() {
            Ok(())
        } else {
            Err(windows::core::Error::from_hresult(hr))
        }
    }
}

impl Drop for PolicyConfig {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ((*self.vtbl).release)(self.raw) };
        }
    }
}

/// Set the given device_id as the default endpoint for Console, Multimedia,
/// and Communications roles. Fails fast on the first role that errors; earlier
/// role changes are NOT rolled back (Windows treats them as independent).
pub(super) fn set_default_for_all_roles(device_id: &str) -> windows::core::Result<()> {
    let pc = PolicyConfig::create()?;
    for role in [eConsole, eMultimedia, eCommunications] {
        pc.set_default_endpoint(device_id, role)?;
    }
    Ok(())
}
