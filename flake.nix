{
  description = "Dioxus development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs";

  outputs = {nixpkgs, ...}: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      config = {
        allowUnfree = true;
        android_sdk.accept_license = true;
      };
    };

    sdk =
      (pkgs.androidenv.composeAndroidPackages {
        platformVersions = ["33" "35"];
        buildToolsVersions = ["34.0.0"];
        # includeEmulator = true;
        includeSystemImages = true;
        systemImageTypes = ["google_apis" "google_apis_playstore"];
        abiVersions = ["arm64-v8a"];
        includeNDK = true;
        ndkVersions = ["27.0.12077973"];
      }).androidsdk;

    nativeBuildInputs = with pkgs; [
      pkg-config
      dioxus-cli
    ];

    buildInputs = with pkgs; [
      atk
      atkmm
      cairo
      fontconfig
      fribidi
      gdk-pixbuf
      glib
      glib-networking
      gtk3
      pango
      gsettings-desktop-schemas
      harfbuzz
      freetype
      libdrm
      libGL
      libgpg-error
      libsoup_3
      mesa
      openssl
      wrapGAppsHook3
      webkitgtk_4_1
      xdotool
      xorg.libX11
      xorg.libxcb
      zlib
      sqlite
      wasm-bindgen-cli
      binaryen
      zenity
      libpng
      libjpeg
      dconf
      shared-mime-info
      librsvg
      gst_all_1.gstreamer
      gst_all_1.gst-plugins-base
      gst_all_1.gst-plugins-good
      gst_all_1.gst-plugins-bad
      gst_all_1.gst-plugins-ugly
      gst_all_1.gst-vaapi
      gst_all_1.gst-libav
      wayland

      nix-ld

      sdk
      openjdk
    ];

    # nix-ld: makes dynamically linked binaries (Android SDK aapt2 etc.) work on NixOS
    nixLdSetup = ''
      export NIX_LD="${pkgs.stdenv.cc.libc}/lib/ld-linux-x86-64.so.2"
      export NIX_LD_LIBRARY_PATH="${pkgs.libglvnd}/lib:${pkgs.stdenv.cc.cc.lib}/lib:${pkgs.zlib}/lib:${pkgs.libGL}/lib"
    '';

    setup = let
      pkgConfigPath =
        builtins.concatStringsSep ":"
        (map (p: "${p.dev or ""}/lib/pkgconfig")
          buildInputs);
    in ''
      export PKG_CONFIG_PATH=${pkgConfigPath}

      export XDG_DATA_DIRS="${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS"

      export ANDROID_HOME="${sdk}/libexec/android-sdk"
      export ANDROID_SDK_ROOT="${sdk}/libexec/android-sdk/"
      export ANDROID_NDK_HOME="${sdk}/libexec/android-sdk/ndk/27.0.12077973"
      export JAVA_HOME="${pkgs.openjdk}"
      export PATH="$PATH:$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools"
      # ${nixLdSetup}
    '';
  in {
    devShells.${system}.default = pkgs.mkShell {
      inherit nativeBuildInputs buildInputs;
      shellHook = setup;
    };

    packages.${system}.default =
      pkgs.rustPlatform.buildRustPackage {
      };
  };
}
