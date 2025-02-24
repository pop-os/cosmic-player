{ pkgs ? import <nixpkgs> {} }:
  let
    libPath = with pkgs; lib.makeLibraryPath [
      libGL
      libxkbcommon
      wayland
    ];
  in {
    devShell = with pkgs; mkShell {
      buildInputs = [
        cargo
        pkg-config
        libxkbcommon
        glib
        gst_all_1.gstreamer
        gst_all_1.gst-plugins-base
        gst_all_1.gst-plugins-good
        gst_all_1.gst-plugins-bad
        gst_all_1.gst-plugins-ugly
        gst_all_1.gst-libav
        gst_all_1.gst-vaapi
        rustc
        rust-analyzer
      ];
      
      RUST_LOG = "debug";
      RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
      LD_LIBRARY_PATH = libPath;
    };
  }
