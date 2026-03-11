{
  description = "cwt — Claude Worktree Manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, rust-overlay, crane, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default;
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

          cwt = craneLib.buildPackage {
            pname = "cwt";
            version = "0.1.0";
            src = craneLib.cleanCargoSource ./.;

            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = with pkgs; [ ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                pkgs.darwin.apple_sdk.frameworks.Security
                pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
              ];

            # Runtime deps that should be on PATH
            postInstall = ''
              wrapProgram $out/bin/cwt \
                --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.git pkgs.tmux ]}
            '';

            nativeBuildInputs = with pkgs; [ pkg-config makeWrapper ];
          };
        in
        {
          default = cwt;
          cwt = cwt;
        }
      );

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" ];
          };
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              rustToolchain
              pkg-config
              git
              tmux
              cargo-watch    # cargo watch -x check
              cargo-edit     # cargo add/rm
            ];

            RUST_LOG = "cwt=debug";

            shellHook = ''
              echo "cwt dev shell — rust $(rustc --version | cut -d' ' -f2)"
              echo "  cargo watch -x check    # continuous type-checking"
              echo "  cargo run -- tui         # launch TUI"
            '';
          };
        }
      );

      overlays.default = final: prev: {
        cwt = self.packages.${prev.system}.default;
      };
    };
}
