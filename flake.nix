{
  description = "Dioxus development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs";

  outputs = {
    self,
    nixpkgs,
    ...
  }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {inherit system;};
  in {
    devShells.${system}.default = pkgs.mkShell {
      buildInputs = with pkgs; [
        pkg-config
        atkmm
        dioxus-cli
        cairo
        fontconfig
        fribidi
        gdk-pixbuf
        glib
        glib-networking
        gtk3
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
      ];
      shellHook = ''
        export PKG_CONFIG_PATH=${
          builtins.concatStringsSep ":" <| map (
            p: "${p.dev or ""}/lib/pkgconfig"
          )
          self.devShells.${system}.default.buildInputs
        }
      '';
    };
  };
}
