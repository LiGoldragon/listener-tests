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
      # The publication crash oracle kills the creation child under strace, so it runs here,
      # with strace on its path, from the plain test build.
      publication-crash =
        let
          tests = (check "listener-tests" [ ]).tests;
          oracle = "a_crash_after_the_record_commits_and_before_publication_leaves_the_final_path_absent_or_complete";
        in
        pkgs.runCommand "listener-tests-publication-crash" { nativeBuildInputs = [ pkgs.strace ]; } ''
          cp -r ${tests.src} "$NIX_BUILD_TOP/source"
          chmod -R u+w "$NIX_BUILD_TOP/source"
          mkdir -p "$(cat ${tests}/target-tmpdir)"
          cd "$NIX_BUILD_TOP/source"
          for binary in ${tests}/tests/*; do
            echo "running $(basename "$binary")"
            "$binary" --ignored --exact --nocapture ${oracle} > "$NIX_BUILD_TOP/run.log" 2>&1 || { cat "$NIX_BUILD_TOP/run.log"; exit 1; }
            cat "$NIX_BUILD_TOP/run.log" | tee -a "$NIX_BUILD_TOP/all.log"
          done
          grep -q "test result: ok. 1 passed" "$NIX_BUILD_TOP/all.log"
          touch $out
        '';
    };
  };
}
