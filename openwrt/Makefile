include $(TOPDIR)/rules.mk

PKG_NAME:=embedded-linux-benchmark
PKG_VERSION:=0.1.0
PKG_RELEASE:=1
PKG_LICENSE:=MIT
PKG_LICENSE_FILES:=LICENSE
PKG_BUILD_DEPENDS:=rust/host

include $(INCLUDE_DIR)/package.mk
include $(TOPDIR)/feeds/packages/lang/rust/rust-package.mk

define Package/embedded-linux-benchmark
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Single-board CPU and RAM copy benchmarks
  URL:=https://github.com/brucerry/embedded-linux-benchmark
  DEPENDS:=$(RUST_ARCH_DEPENDS) +libgcc
endef

define Package/embedded-linux-benchmark/description
  Measure CPU integer throughput and bounded RAM copy throughput on one board.
endef

define Build/Compile
	$(call Build/Compile/Cargo,,--offline)
endef

$(eval $(call RustBinPackage,embedded-linux-benchmark))
$(eval $(call BuildPackage,embedded-linux-benchmark))
