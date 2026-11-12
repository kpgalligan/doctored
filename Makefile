APP := Doctored
BUILD_DIR := build
APP_PATH := $(BUILD_DIR)/Build/Products/Release/$(APP).app
DIST_PATH := $(BUILD_DIR)/Build/Products/Distribution/$(APP).app

.PHONY: all app app-debug rust test t test-serial gate cancel-bench lint typecheck project run clean \
	agent agent-test mcp-test dist release-check

all: app

rust:
	cd core && cargo build --release

# The whole suite: the standalone test executables four at a time with the
# serial suite beside them, the other suites one at a time, then the solo list.
test:
	python3 scripts/run_tests.py

# One test file, whichever binary holds it:
#   make t F=<file> [T=<filter>] [A="<libtest flags>"]
# T is the start of the test's path inside the file (`module::name` for a test
# in an inline module); a standalone file also takes any substring, as libtest
# does. A run of zero tests fails.
t:
	python3 scripts/run_test_file.py "$(F)" "$(T)" $(A)

test-serial:
	cd core && cargo test --release

# The one Full verification command (CLAUDE.md "Verification"); GATE_ARGS="--force" runs every step.
gate:
	python3 scripts/gate.py $(GATE_ARGS)

# The ignored wall-clock probes (`#[ignore = "make cancel-bench: …"]`):
# ignored in `make test`; release, serial, every file runs even after a panic;
# the output is the measurement record.
CANCEL_BENCH_FILES := \
	regional_byte_preparation_cancel \
	regional_group_materialization_cancel \
	regional_byte_primitive_cancel \
	regional_style_preparation_cancel \
	regional_typed_preparation_cancel \
	regional_typed_styled_composite_cancel \
	regional_context_preparation_cancel \
	regional_sampling_fallback_cancel \
	regional_sampling_tree_cancel \
	bounded_selection_support \
	bounded_artwork_coverage \
	bounded_brush_footprint \
	tile_store \
	layer_raster_compare \
	soft_proof_tests \
	heal_job \
	byte_geometry_cancel \
	edge_trace \
	closed_form_matting \
	foreground_estimation \
	job_control \
	precision_adjustments \
	precision_color \
	precision_conversion \
	precision_cutout \
	precision_geometry \
	precision_healing \
	precision_inpaint \
	precision_masks \
	precision_prototype \
	precision_redeye \
	precision_retouch_progress \
	precision_scalar_selection \
	precision_stats \
	refinement_cutout \
	sampled_matting \
	selection_tools \
	tiled_soft_matting \
	trimap_strokes \
	content_aware_cost_tests \
	content_aware_tests \
	heal_tests \
	lut_tests \
	metadata_packet_framing \
	metadata_tests \
	style_cache \
	regional_groups \
	regional_masks_clips \
	regional_precision_masks_clips \
	regional_byte_clip_guide \
	regional_byte_effects \
	regional_byte_guide \
	regional_byte_knockout \
	regional_byte_nested_guide \
	regional_byte_pass_through_guide \
	regional_byte_styled_clip_guide \
	regional_byte_styled_groups \
	style_inner_glow_aa_parity \
	export_codec_cancel

cancel-bench:
	@failed=0; for file in $(CANCEL_BENCH_FILES); do \
		python3 scripts/run_test_file.py $$file "" --ignored --test-threads=1 --nocapture || failed=1; \
	done; exit $$failed

# The kernel-module layering check (core/CLAUDE.md "Layers"), then clippy on
# the core and on the doctored-mcp helper (mcp-proxy/).
lint:
	python3 scripts/check_layers.py
	cd core && cargo clippy --all-targets -- -D warnings
	cd mcp-proxy && cargo clippy --all-targets -- -D warnings

# The doctored-mcp stdio helper's own suite (mcp-proxy/tests/stdio.rs).
mcp-test:
	cd mcp-proxy && cargo test

# The assistant's helper alone, through the script the app build's "Build Agent Helper"
# phase runs: stripped and ad-hoc signed as the bundle's copy is, at build/agent/doctored-agent
# (for DR_ASSISTANT_HELPER, or a stdio drive with no app build). DR_AGENT_BUILD_CONFIG as there.
agent:
	bash scripts/build_agent_helper.sh build/agent/doctored-agent

# The assistant helper's own suite (agent/Tests; no network, no real key), on the scratch
# path the app build's helper uses; the gate's `agent-tests` step. DR_AGENT_STRICT=1 holds the
# helper's own targets to -warnings-as-errors (agent/Package.swift); the shipping build is not.
agent-test:
	DR_AGENT_STRICT=1 swift test --package-path agent --scratch-path build/agent-swift

project:
	DR_BASE_VERSION="$$(cat VERSION)" xcodegen generate

app: rust project
	xcodebuild -project $(APP).xcodeproj -scheme $(APP) -configuration Release \
		-derivedDataPath $(BUILD_DIR) build

DEBUG_XCFLAGS := COMPILER_INDEX_STORE_ENABLE=NO ENABLE_DEBUG_DYLIB=NO

app-debug: rust project
	xcodebuild -project $(APP).xcodeproj -scheme $(APP) -configuration Debug \
		-derivedDataPath $(BUILD_DIR) $(DEBUG_XCFLAGS) build

# The Distribution configuration the publish workflow archives (Release plus Sparkle,
# designs/sparkle-updates.md), ad-hoc signed beside Release's product and never launched
# as built; then scripts/updater_check.py over both (its D9 launches only a scratch copy
# re-signed without the runtime, under automation) and its mutation cases, both run even
# when the first fails. Run it after `make gate` passes, never while a gate runs: it
# rebuilds Release first. DIST_ARGS passes extra build settings to xcodebuild.
dist: app
	xcodebuild -project $(APP).xcodeproj -scheme $(APP) -configuration Distribution \
		-derivedDataPath $(BUILD_DIR) $(DIST_ARGS) build
	rc=0; python3 scripts/updater_check.py dist --release "$(APP_PATH)" --dist "$(DIST_PATH)" || rc=1; \
	python3 scripts/updater_check.py mutants --release "$(APP_PATH)" --dist "$(DIST_PATH)" || rc=1; exit $$rc

# The release tooling's own checks; no build, no network, no key.
release-check:
	python3 scripts/appcast.py --selftest
	python3 scripts/updater_check.py --selftest

# Fast compile check of the Swift sources without a full Xcode build.
typecheck:
	swiftc -typecheck -enable-batch-mode -j "$$(sysctl -n hw.logicalcpu)" \
		-sdk "$$(xcrun --show-sdk-path --sdk macosx)" \
		-target arm64-apple-macos15.0 \
		-import-objc-header app/Bridging/Doctored-Bridging-Header.h \
		app/Sources/*.swift

run: app
	open "$(APP_PATH)"

clean:
	rm -rf $(BUILD_DIR) $(APP).xcodeproj app/Info.plist
	cd core && cargo clean
