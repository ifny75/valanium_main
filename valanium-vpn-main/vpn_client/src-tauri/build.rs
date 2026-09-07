fn main() {
    #[cfg(target_os = "windows")]
    {
        // wireguard.dll (wireguard-nt) needs to sit next to the built exe —
        // both for `cargo tauri dev` (copy into target/{debug,release}) and
        // for installed builds (tauri.conf.json's bundle.resources handles
        // that copy). LoadLibrary in wg_nt.rs resolves it relative to
        // current_exe(), so this just has to land in the same directory.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let src = std::path::Path::new(manifest_dir).join("bin/wireguard.dll");
        for profile in ["debug", "release"] {
            let dest_dir = std::path::Path::new(manifest_dir).join("target").join(profile);
            if dest_dir.exists() {
                let _ = std::fs::copy(&src, dest_dir.join("wireguard.dll"));
            }
        }
        println!("cargo:rerun-if-changed=bin/wireguard.dll");
    }

    #[cfg(target_os = "windows")]
    {
        // Real Connect creates a wireguard-nt virtual network adapter and
        // changes the system routing table — both need admin rights, so the
        // whole app requests elevation on launch (same pattern other VPN
        // clients use) rather than prompting mid-session.
        //
        // The <dependency> block below is NOT optional decoration — it's
        // Tauri's own default manifest content (WindowsAttributes::new()
        // bakes it in via windows-app-manifest.xml). Supplying a custom
        // app_manifest() replaces that default wholesale, and without this
        // block Windows binds the process to the ancient side-by-side
        // comctl32.dll instead of v6, which doesn't export TaskDialogIndirect
        // — WebView2/Tauri needs that symbol and fails at launch with
        // "Entry Point Not Found" if it's missing.
        let windows = tauri_build::WindowsAttributes::new().app_manifest(
            r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
      <security>
          <requestedPrivileges>
              <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
          </requestedPrivileges>
      </security>
  </trustInfo>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#,
        );
        let attrs = tauri_build::Attributes::new().windows_attributes(windows);
        tauri_build::try_build(attrs).expect("failed to run tauri build script");
    }

    #[cfg(not(target_os = "windows"))]
    tauri_build::build();
}
