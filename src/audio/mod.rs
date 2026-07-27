//! Windows Core Audio wrapper.
//!
//! Public API kept small on purpose so the HTTP server does not need to know
//! about COM or unsafe Windows API details.

use windows::core::PWSTR;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, ERole, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, DEVICE_STATE, DEVICE_STATE_ACTIVE,
    DEVICE_STATE_DISABLED, DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Foundation::{BOOL, E_INVALIDARG};
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

/// Master volume state for the current default Multimedia render endpoint.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MasterVolume {
    pub device_id: String,
    pub level: f32,
    pub muted: bool,
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

const INVALID_VOLUME_LEVEL_CONTEXT: &str = "invalid master volume level";

impl AudioError {
    pub fn is_invalid_input(&self) -> bool {
        self.context == INVALID_VOLUME_LEVEL_CONTEXT
    }
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

fn invalid_volume_level() -> AudioError {
    AudioError {
        context: INVALID_VOLUME_LEVEL_CONTEXT,
        source: windows::core::Error::from_hresult(E_INVALIDARG),
    }
}

fn validate_volume_level(level: f32) -> Result<()> {
    if level.is_finite() && (0.0..=1.0).contains(&level) {
        Ok(())
    } else {
        Err(invalid_volume_level())
    }
}

fn default_multimedia_volume() -> Result<(String, IAudioEndpointVolume)> {
    let enumerator = create_enumerator()?;
    let device = wrap("GetDefaultAudioEndpoint(eRender, eMultimedia)", unsafe {
        enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)
    })?;
    let id = device_id(&device)?;
    let endpoint = wrap("IMMDevice::Activate(IAudioEndpointVolume)", unsafe {
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
    })?;
    Ok((id, endpoint))
}

fn read_master_volume(device_id: String, endpoint: &IAudioEndpointVolume) -> Result<MasterVolume> {
    let level = wrap(
        "IAudioEndpointVolume::GetMasterVolumeLevelScalar",
        unsafe { endpoint.GetMasterVolumeLevelScalar() },
    )?;
    let muted = wrap("IAudioEndpointVolume::GetMute", unsafe { endpoint.GetMute() })?.0 != 0;
    Ok(MasterVolume {
        device_id,
        level,
        muted,
    })
}

/// Read the master level and mute state from the default Multimedia endpoint.
pub fn get_master_volume() -> Result<MasterVolume> {
    let _com = ComGuard::new()?;
    let (id, endpoint) = default_multimedia_volume()?;
    read_master_volume(id, &endpoint)
}

/// Update either or both master-volume fields and return the resulting state.
/// This is kept crate-visible so the HTTP layer can apply a combined request
/// in one endpoint activation without exposing COM types.
pub(crate) fn update_master_volume(level: Option<f32>, muted: Option<bool>) -> Result<MasterVolume> {
    if level.is_none() && muted.is_none() {
        return Err(invalid_volume_level());
    }
    if let Some(level) = level {
        validate_volume_level(level)?;
    }

    let _com = ComGuard::new()?;
    let (id, endpoint) = default_multimedia_volume()?;
    if let Some(level) = level {
        wrap(
            "IAudioEndpointVolume::SetMasterVolumeLevelScalar",
            unsafe { endpoint.SetMasterVolumeLevelScalar(level, std::ptr::null()) },
        )?;
    }
    if let Some(muted) = muted {
        wrap(
            "IAudioEndpointVolume::SetMute",
            unsafe { endpoint.SetMute(BOOL(muted as i32), std::ptr::null()) },
        )?;
    }
    read_master_volume(id, &endpoint)
}

/// Set the master level on the default Multimedia endpoint.
pub fn set_master_volume(level: f32) -> Result<MasterVolume> {
    update_master_volume(Some(level), None)
}

/// Set the mute state on the default Multimedia endpoint.
pub fn set_master_mute(muted: bool) -> Result<MasterVolume> {
    update_master_volume(None, Some(muted))
}
