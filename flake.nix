# A Nix dev shell for Linux and macOS: `nix develop`, or `use flake` in a local .envrc with
# direnv. It mirrors Mainline's dev shell. Nothing in the build reads it: the toolchain pins are
# rust-toolchain.toml, package.json's `packageManager` and `engines`, and the CI workflows install
# the same versions without Nix.
#
# Rust comes from rustup, which reads rust-toolchain.toml, so the shell and CI build with the
# same compiler. Node is the LTS line `.node-version` names. Playwright's browsers come from
# nixpkgs (`playwright-driver.browsers`), so `@playwright/test` in package.json must stay at the
# version of `playwright-driver` here. The Linux libraries are what Tauri 2 links (WebKitGTK 4.1,
# libsoup 3, GTK 3, librsvg).
{
  description = "Stepclimb dev shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in
    {
      devShells = forAll (
        pkgs:
        let
          inherit (pkgs) lib;
          inherit (pkgs.stdenv.hostPlatform) isLinux;
        in
        {
          default = pkgs.mkShell {
            packages =
              with pkgs;
              [
                rustup
                pkg-config
                just
                cargo-nextest
                cargo-deny
                sqlx-cli
                gitleaks
                python3
                nodejs_26
                pnpm
                playwright-driver.browsers
                git
              ]
              ++ lib.optionals isLinux [
                webkitgtk_4_1
                libsoup_3
                gtk3
                librsvg
                openssl
              ];
            PLAYWRIGHT_BROWSERS_PATH = "${pkgs.playwright-driver.browsers}";
            PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
            # Rootless Docker listens on the user's socket, as in Mainline's shell.
            shellHook = lib.optionalString isLinux ''
              export DOCKER_HOST="''${DOCKER_HOST:-unix://''${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/docker.sock}"
            '';
          };
        }
      );
    };
}
