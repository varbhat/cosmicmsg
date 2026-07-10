{pkgs ? import <nixpkgs> {}, ...}:
pkgs.mkShell {
  strictDeps = true;

  nativeBuildInputs = with pkgs; [
    cargo
    clippy
    rustc
    rustfmt
    pkg-config
  ];

  buildInputs = with pkgs; [
    libxkbcommon
    wayland
  ];

  LD_LIBRARY_PATH = with pkgs;
    lib.makeLibraryPath [
      libxkbcommon
      wayland
    ];
}
