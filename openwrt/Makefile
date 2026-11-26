include $(TOPDIR)/rules.mk

PKG_NAME:=eld-engine
PKG_VERSION:=0.2.1
PKG_RELEASE:=1
PKG_LICENSE:=MIT
PKG_LICENSE_FILES:=LICENSE

include $(INCLUDE_DIR)/package.mk

OPENWRT_RUST_TARGET ?=
OPENWRT_BUILD_STD ?= 0

define Package/eld-engine
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Component benchmarks for embedded Linux
  URL:=https://github.com/brucerry/embedded-linux-diagnostic-engine
  DEPENDS:=+libgcc
endef

define Package/eld-engine/description
  Measure Linux components with bounded workloads and opt-in fixture tests.
endef

define Build/Compile
	@test -n "$(OPENWRT_RUST_TARGET)" || \
		{ echo "Set OPENWRT_RUST_TARGET to the Rust target matching this SDK" >&2; exit 1; }
	@case "$(OPENWRT_BUILD_STD)" in \
		0|1) ;; \
		*) echo "Set OPENWRT_BUILD_STD to 0 or 1" >&2; exit 1 ;; \
	esac
	cd "$(PKG_BUILD_DIR)" && \
		CARGO_TARGET_DIR="$(PKG_BUILD_DIR)/target" \
		RUSTFLAGS="-C linker=$(TARGET_CC_NOCACHE) -C target-feature=-crt-static" \
		cargo build $(if $(filter 1,$(OPENWRT_BUILD_STD)),-Z build-std) \
			--release --locked --offline --target "$(OPENWRT_RUST_TARGET)"
endef

define Package/eld-engine/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/target/$(OPENWRT_RUST_TARGET)/release/eld-engine $(1)/usr/bin/
endef

$(eval $(call BuildPackage,eld-engine))
