//! Windows Core Audio wrapper.
//!
//! Public API kept small on purpose so C3 (HTTP server) only needs
//! `list_devices()` and `set_default(device_id)`. Serialization is added in C3
//! when the axum layer needs it; C2 stays framework-free.

use windows::core::PWSTR;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATE_DISABLED,
    DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};

mod policyconfig;

/// One playback endpoint on the host.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub state: DeviceState,
    pub is_default_console: bool,
    pub is_default_multimedia: bool,
    pub is_default_communications: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Active,
    Unplugged,
    Disabled,
    NotPresent,
    Unknown,
}

impl DeviceState {
    fn from_raw(raw: DEVICE_STATE) -> Self {
        match raw {
            DEVICE_STATE_ACTIVE => Self::Active,
            DEVICE_STATE_UNPLUGGED => Self::Unplugged,
            DEVICE_STATE_DISABLED => Self::Disabled,
            DEVICE_STATE_NOTPRESENT => Self::NotPresent,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Unplugged => "unplugged",
            Self::Disabled => "disabled",
            Self::NotPresent => "not_present",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug)]
pub struct AudioError {
    pub context: &'static str,
    pub source: windows::core::Error,
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.context, self.source)
    }
}
impl std::error::Error for AudioError {}

type Result<T> = std::result::Result<T, AudioError>;

fn wrap<T>(ctx: &'static str, r: windows::core::Result<T>) -> Result<T> {
    r.map_err(|e| AudioError { context: ctx, source: e })
}

/// RAII guard for CoInitializeEx / CoUninitialize (MTA). Nested creation is
/// safe: Windows refcounts CoInitialize per thread.
struct ComGuard;

impl ComGuard {
    fn new() -> Result<Self> {
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() {
                return Err(AudioError {
                    context: "CoInitializeEx",
                    source: windows::core::Error::from_hresult(hr),
                });
            }
        }
        Ok(Self)
    }
}
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() }
    }
}

fn create_enumerator() -> Result<IMMDeviceEnumerator> {
    wrap("CoCreateInstance(MMDeviceEnumerator)", unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
    })
}

fn pwstr_to_string(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0usize;
        while *p.0.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p.0, len))
    }
}

fn device_id(dev: &IMMDevice) -> Result<String> {
    let raw = wrap("IMMDevice::GetId", unsafe { dev.GetId() })?;
    Ok(pwstr_to_string(raw))
}

fn device_state(dev: &IMMDevice) -> Result<DEVICE_STATE> {
    wrap("IMMDevice::GetState", unsafe { dev.GetState() })
}

fn device_friendly_name(dev: &IMMDevice) -> Result<String> {
    let store = wrap("IMMDevice::OpenPropertyStore", unsafe {
        dev.OpenPropertyStore(STGM_READ)
    })?;
    let prop = wrap("IPropertyStore::GetValue(FriendlyName)", unsafe {
        store.GetValue(&PKEY_Device_FriendlyName)
    })?;
    // windows-core 0.58 exposes PROPVARIANT as an opaque type; its Display impl
    // formats VT_LPWSTR / VT_BSTR as the underlying string. Non-string variants
    // would render as "<vt=N>" which is fine for a fallback label.
    Ok(prop.to_string())
}

/// Enumerate all endpoints (active / unplugged / disabled) and mark which one
/// is the current default per ERole.
pub fn list_devices() -> Result<Vec<AudioDevice>> {
    let _com = ComGuard::new()?;
    let enumerator = create_enumerator()?;

    let mask = DEVICE_STATE_ACTIVE.0 | DEVICE_STATE_UNPLUGGED.0 | DEVICE_STATE_DISABLED.0;
    let collection = wrap("EnumAudioEndpoints", unsafe {
        enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE(mask))
    })?;
    let count = wrap("IMMDeviceCollection::GetCount", unsafe { collection.GetCount() })?;

    let default_ids = current_defaults(&enumerator);

    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let dev = wrap("IMMDeviceCollection::Item", unsafe { collection.Item(i) })?;
        let id = device_id(&dev)?;
        let name = device_friendly_name(&dev).unwrap_or_else(|_| "(unnamed)".to_string());
        let state = DeviceState::from_raw(device_state(&dev)?);
        out.push(AudioDevice {
            is_default_console: default_ids.console.as_deref() == Some(&id),
            is_default_multimedia: default_ids.multimedia.as_deref() == Some(&id),
            is_default_communications: default_ids.communications.as_deref() == Some(&id),
            id,
            name,
            state,
        });
    }
    Ok(out)
}

#[derive(Default)]
struct DefaultIds {
    console: Option<String>,
    multimedia: Option<String>,
    communications: Option<String>,
}

fn current_defaults(enumerator: &IMMDeviceEnumerator) -> DefaultIds {
    fn one(enumerator: &IMMDeviceEnumerator, role: ERole) -> Option<String> {
        unsafe {
            let dev = enumerator.GetDefaultAudioEndpoint(eRender, role).ok()?;
            let p = dev.GetId().ok()?;
            Some(pwstr_to_string(p))
        }
    }
    DefaultIds {
        console: one(enumerator, eConsole),
        multimedia: one(enumerator, eMultimedia),
        communications: one(enumerator, eCommunications),
    }
}

/// Switch the default output device for all three roles (Console / Multimedia
/// / Communications) at once. Uses non-public IPolicyConfig; tries Win10/11
/// IID first, falls back to the Vista IID.
pub fn set_default(device_id: &str) -> Result<()> {
    let _com = ComGuard::new()?;
    policyconfig::set_default_for_all_roles(device_id).map_err(|e| AudioError {
        context: "IPolicyConfig::SetDefaultEndpoint",
        source: e,
    })
}
