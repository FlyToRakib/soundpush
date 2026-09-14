//! Firewall and network profile checks, and the one-click firewall fix (plan §8.1 "Windows
//! firewall blocks / network set to Public", §13.6).
//!
//! SoundPush never runs elevated. Checking reads the Windows Firewall policy and the network
//! list as a normal user. Fixing starts Windows' own `netsh` through a UAC prompt, which replaces
//! SoundPush's inbound rules with one rule scoped to this executable, UDP only, for private and
//! domain networks (and public networks only when the user asks for that).
//! The rule is named [`RULE_NAME`]; the uninstaller removes it (`windows/hooks.nsh`).

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    /// The checks are available on this platform.
    pub supported: bool,
    /// The Windows Firewall is on for a network this computer is connected to.
    pub firewall_enabled: bool,
    /// Other devices cannot connect: no allow rule for SoundPush covers the current network,
    /// or a block rule (created when the Windows Firewall prompt was dismissed) applies.
    pub blocked: bool,
    /// A connected network uses the Public profile.
    pub public_network: bool,
    /// Name of the public network, for the explanation.
    pub public_network_name: Option<String>,
    /// SoundPush's own rule exists and includes public networks.
    pub allowed_on_public: bool,
}

/// Name of the inbound rule SoundPush adds (Windows Firewall only).
#[cfg_attr(not(windows), allow(dead_code))]
pub const RULE_NAME: &str = "SoundPush";

pub fn status() -> NetworkStatus {
    #[cfg(windows)]
    return windows::status();
    #[allow(unreachable_code)]
    NetworkStatus::default()
}

/// Add the firewall rule through a UAC prompt. Blocks until the prompt is answered.
/// Returns `Err("cancelled")` when the user declines.
pub fn fix_firewall(include_public: bool) -> Result<(), String> {
    #[cfg(windows)]
    return windows::fix(include_public);
    #[allow(unreachable_code)]
    {
        let _ = include_public;
        Err("not needed on this platform".into())
    }
}

#[cfg(windows)]
mod windows {
    use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, VARIANT_BOOL};
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        INetFwPolicy2, INetFwRule, NET_FW_ACTION_ALLOW, NET_FW_ACTION_BLOCK,
        NET_FW_IP_PROTOCOL_ANY, NET_FW_IP_PROTOCOL_UDP, NET_FW_PROFILE_TYPE2,
        NET_FW_PROFILE2_PUBLIC, NET_FW_RULE_DIR_IN, NetFwPolicy2,
    };
    use windows::Win32::Networking::NetworkListManager::{
        INetwork, INetworkListManager, NLM_ENUM_NETWORK_CONNECTED, NLM_NETWORK_CATEGORY_PUBLIC,
        NetworkListManager,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::System::Ole::IEnumVARIANT;
    use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HSTRING, IUnknown, Interface, PCWSTR, VARIANT, w};

    use super::{NetworkStatus, RULE_NAME};

    fn with_com<T>(f: impl FnOnce() -> T) -> T {
        // SAFETY: balanced with CoUninitialize when initialisation succeeded.
        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let result = f();
        if init.is_ok() {
            // SAFETY: matches the successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
        result
    }

    fn truthy(v: windows::core::Result<VARIANT_BOOL>) -> bool {
        v.is_ok_and(|b| b.as_bool())
    }

    /// Paths in firewall rules may use environment variables and any letter case.
    fn same_program(rule_path: &str, exe: &str) -> bool {
        let mut expanded = rule_path.to_string();
        for (key, value) in std::env::vars() {
            let pattern = format!("%{key}%");
            if expanded
                .to_ascii_lowercase()
                .contains(&pattern.to_ascii_lowercase())
            {
                let start = expanded
                    .to_ascii_lowercase()
                    .find(&pattern.to_ascii_lowercase())
                    .unwrap_or(0);
                expanded.replace_range(start..start + pattern.len(), &value);
            }
        }
        expanded.eq_ignore_ascii_case(exe)
    }

    fn connected_networks() -> Vec<(bool, String)> {
        // SAFETY: COM calls on objects created and released in this scope.
        unsafe {
            let Ok(list) =
                CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL)
            else {
                return Vec::new();
            };
            let Ok(networks) = list.GetNetworks(NLM_ENUM_NETWORK_CONNECTED) else {
                return Vec::new();
            };
            let mut out = Vec::new();
            loop {
                let mut item: [Option<INetwork>; 1] = [None];
                let mut fetched = 0u32;
                if networks.Next(&mut item, Some(&mut fetched)).is_err() || fetched == 0 {
                    break;
                }
                let Some(network) = item[0].take() else { break };
                let public = network
                    .GetCategory()
                    .is_ok_and(|c| c == NLM_NETWORK_CATEGORY_PUBLIC);
                let name = network.GetName().map(|n| n.to_string()).unwrap_or_default();
                out.push((public, name));
            }
            out
        }
    }

    pub fn status() -> NetworkStatus {
        let Ok(exe) = std::env::current_exe() else {
            return NetworkStatus::default();
        };
        let exe = exe.to_string_lossy().to_string();
        with_com(|| {
            let networks = connected_networks();
            let public = networks.iter().find(|(public, _)| *public);
            // SAFETY: COM calls on objects created and released in this scope.
            let firewall = unsafe {
                (|| -> windows::core::Result<(bool, bool, bool)> {
                    let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_ALL)?;
                    let profiles = policy.CurrentProfileTypes()?;
                    let active = [1, 2, 4].into_iter().filter(|p| profiles & p != 0);
                    let enabled: Vec<i32> = active
                        .filter(|p| truthy(policy.get_FirewallEnabled(NET_FW_PROFILE_TYPE2(*p))))
                        .collect();
                    if enabled.is_empty() {
                        return Ok((false, false, false));
                    }
                    let shields_up = enabled.iter().any(|p| {
                        truthy(policy.get_BlockAllInboundTraffic(NET_FW_PROFILE_TYPE2(*p)))
                    });

                    // Profiles covered by an enabled inbound allow rule for this program (UDP or any
                    // protocol), and whether a block rule for it applies to a current profile.
                    let mut allowed = 0i32;
                    let mut block = false;
                    let mut public_rule = false;
                    let rules = policy.Rules()?;
                    let enumerator: IEnumVARIANT = rules._NewEnum()?.cast()?;
                    loop {
                        let mut item = [VARIANT::default()];
                        let mut fetched = 0u32;
                        if enumerator.Next(&mut item, &mut fetched).is_err() || fetched == 0 {
                            break;
                        }
                        let Ok(rule) =
                            IUnknown::try_from(&item[0]).and_then(|u| u.cast::<INetFwRule>())
                        else {
                            continue;
                        };
                        let Ok(app) = rule.ApplicationName() else {
                            continue;
                        };
                        if app.is_empty() || !same_program(&app.to_string(), &exe) {
                            continue;
                        }
                        let inbound = rule.Direction().is_ok_and(|d| d == NET_FW_RULE_DIR_IN);
                        let protocol = rule.Protocol().unwrap_or(0);
                        if !inbound
                            || !truthy(rule.Enabled())
                            || (protocol != NET_FW_IP_PROTOCOL_UDP.0
                                && protocol != NET_FW_IP_PROTOCOL_ANY.0)
                        {
                            continue;
                        }
                        let rule_profiles = rule.Profiles().unwrap_or(0);
                        match rule.Action() {
                            Ok(a) if a == NET_FW_ACTION_ALLOW => {
                                allowed |= rule_profiles;
                                if rule.Name().is_ok_and(|n| n == RULE_NAME)
                                    && rule_profiles & NET_FW_PROFILE2_PUBLIC.0 != 0
                                {
                                    public_rule = true;
                                }
                            }
                            Ok(a) if a == NET_FW_ACTION_BLOCK => {
                                block |= enabled.iter().any(|p| rule_profiles & p != 0);
                            }
                            _ => {}
                        }
                    }
                    let uncovered = enabled.iter().any(|p| allowed & p == 0);
                    Ok((true, shields_up || block || uncovered, public_rule))
                })()
            };
            let (firewall_enabled, blocked, allowed_on_public) = firewall.unwrap_or_else(|e| {
                tracing::warn!(error = %e, "could not read the firewall policy");
                (false, false, false)
            });
            NetworkStatus {
                supported: true,
                firewall_enabled,
                blocked,
                public_network: public.is_some(),
                public_network_name: public
                    .map(|(_, name)| name.clone())
                    .filter(|n| !n.is_empty()),
                allowed_on_public,
            }
        })
    }

    /// `netsh` arguments that replace this program's inbound rules with SoundPush's rule.
    pub(super) fn commands(exe: &str, include_public: bool) -> String {
        let profiles = if include_public {
            "private,domain,public"
        } else {
            "private,domain"
        };
        // Deleting by program also removes the block rules Windows adds when its firewall prompt
        // is dismissed; netsh fails that step harmlessly when there is nothing to delete.
        format!(
            "netsh advfirewall firewall delete rule name=all dir=in program=\"{exe}\" & \
             netsh advfirewall firewall delete rule name=\"{RULE_NAME}\" & \
             netsh advfirewall firewall add rule name=\"{RULE_NAME}\" dir=in action=allow protocol=UDP \
             program=\"{exe}\" profile={profiles} enable=yes"
        )
    }

    pub fn fix(include_public: bool) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.to_string_lossy().to_string();
        if exe.contains('"') {
            return Err("unexpected program path".into());
        }
        let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let cmd = HSTRING::from(format!(r"{system}\System32\cmd.exe"));
        // /s strips only the outer quotes, so the quoted program path inside stays intact.
        let parameters = HSTRING::from(format!("/d /s /c \"{}\"", commands(&exe, include_public)));
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(cmd.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: `info` and the strings it points to outlive the call; the process handle is
        // waited on and closed below.
        unsafe {
            if let Err(e) = ShellExecuteExW(&mut info) {
                return Err(if e.code() == ERROR_CANCELLED.to_hresult() {
                    "cancelled".into()
                } else {
                    e.message()
                });
            }
            if info.hProcess.is_invalid() {
                return Err("the firewall helper did not start".into());
            }
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 1u32;
            let exited = GetExitCodeProcess(info.hProcess, &mut code);
            let _ = CloseHandle(info.hProcess);
            exited.map_err(|e| e.message())?;
            if code == 0 {
                Ok(())
            } else {
                Err(format!("netsh exited with code {code}"))
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fix_is_scoped_to_the_program_and_private_networks() {
            let c = commands(r"C:\Users\A&B\SoundPush\soundpush.exe", false);
            assert!(c.contains(r#"program="C:\Users\A&B\SoundPush\soundpush.exe""#));
            assert!(c.contains("protocol=UDP"));
            assert!(c.contains("profile=private,domain enable=yes"));
            assert!(commands("x.exe", true).contains("profile=private,domain,public"));
        }

        #[test]
        fn rule_paths_expand_environment_variables() {
            if let Ok(root) = std::env::var("SystemRoot") {
                assert!(same_program(
                    r"%SystemRoot%\x.exe",
                    &format!(r"{root}\x.exe")
                ));
                assert!(same_program(
                    &format!(r"{}\X.EXE", root.to_uppercase()),
                    &format!(r"{root}\x.exe")
                ));
            }
        }

        #[test]
        fn status_reads_without_admin() {
            assert!(status().supported);
        }
    }
}
