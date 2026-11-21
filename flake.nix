{
  description = "Independent Listener contract and fresh Memory scenarios";
  inputs.base.url = "github:LiGoldragon/build-base";
  outputs = { self, base }: base.lib.workspaceOutputs {
    cargoNix = ./Cargo.nix;
    memberChecks = { check, ... }: { listener-tests = check "listener-tests" [ "datom" ]; };
    extraChecks = { pkgs, toolchain, check, ... }: {
      plain = check "listener-tests" [];
      fmt = pkgs.runCommand "listener-tests-format" { nativeBuildInputs = [ toolchain ]; } ''
        rustfmt --edition 2024 --check ${./src}/lib.rs ${./tests}/scenarios.rs
        touch $out
      '';
    };
  };
}
