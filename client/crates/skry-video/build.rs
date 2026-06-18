//! En Windows, el FFmpeg estático (vcpkg) depende de varias libs de sistema que
//! la discovery de ffmpeg-sys/vcpkg no propaga al linker (DirectShow de
//! avdevice, schannel/NCrypt de avformat-tls, etc.). Se enlazan acá; sin esto el
//! link falla con decenas de "unresolved external symbol".

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let system_libs = [
            "strmiids", // DirectShow IIDs (avdevice/dshow)
            "ncrypt",   // NCrypt* (avformat tls_schannel)
            "crypt32",  // Cert*/Crypt* (schannel)
            "secur32",  // schannel
            "bcrypt",   // crypto
            "mfplat",   // Media Foundation
            "mfuuid",   // Media Foundation GUIDs
            "ole32", "oleaut32", "user32", "gdi32", "ws2_32", // sockets
            "advapi32", "shell32", "vfw32", // Video for Windows
            "psapi",
        ];
        for lib in system_libs {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}
