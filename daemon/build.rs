fn main() {
    // FFmpeg's avcodec static library uses Windows Media Foundation for
    // hardware encoding (mfenc / mf_utils). The MF COM interface IIDs
    // live in these Windows SDK libraries which the linker doesn't pull
    // in automatically when linking a static FFmpeg build.
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=mfuuid");   // IID_IMFTransform, IID_IMFMediaEventGenerator, …
        println!("cargo:rustc-link-lib=strmiids"); // IID_ICodecAPI
    }
}
