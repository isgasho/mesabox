#!/bin/bash -e

export COVERAGE_OPTIONS="-Zprofile -Copt-level=1 -Clink-dead-code -Ccodegen-units=1 -Zno-landing-pads"
export RUSTC_WRAPPER="./util/cov-rustc"
export CARGO_INCREMENTAL=0

LCOVOPT="--gcov-tool ./util/llvm-gcov --rc lcov_branch_coverage=1 --rc lcov_excl_line=assert"

# cleanup all
rm -rf *.info *.gcda *.gcno
cargo clean

# unit tests
cargo rustc --all-features --profile test --lib
rm ./target/debug/mesabox-*.d
./target/debug/mesabox-*
lcov ${LCOVOPT} --capture --directory . --base-directory . -o mesabox.info

# cleanup target
cargo clean

# integration tests
cargo rustc --all-features --test tests
rm ./target/debug/tests-*.d
./target/debug/tests-*
lcov ${LCOVOPT} --capture --directory . --base-directory . -o tests.info

# combining and filtering
lcov ${LCOVOPT} --add mesabox.info --add tests.info -o coverage.info
lcov ${LCOVOPT} --extract coverage.info `find "$(cd src; pwd)" -name "*.rs"` -o final.info

# generate report
genhtml --branch-coverage --demangle-cpp --legend final.info -o target/coverage/ --ignore-errors source
