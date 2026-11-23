/* Compile with each Linux SDK's C compiler to verify the Rust FFI assumptions. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/gpio.h>
#include <linux/i2c-dev.h>
#include <linux/i2c.h>
#include <linux/spi/spidev.h>
#include <stddef.h>

#define CHECK(condition) _Static_assert(condition, #condition)
#define FIELD(type, field, offset) CHECK(offsetof(struct type, field) == offset)

CHECK(EBUSY == 16);
CHECK(EINVAL == 22);
CHECK(ENOTTY == 25);
CHECK(I2C_SLAVE == 0x0703);
CHECK(I2C_FUNCS == 0x0705);
CHECK(I2C_FUNC_I2C == 1);

#if defined(__mips__)
CHECK(O_NONBLOCK == 0x0080 && O_NOCTTY == 0x0800);
CHECK(SPI_IOC_MESSAGE(1) == 0x80206b00);
CHECK(SPI_IOC_RD_BITS_PER_WORD == 0x40016b03);
CHECK(SPI_IOC_RD_MAX_SPEED_HZ == 0x40046b04);
CHECK(SPI_IOC_RD_MODE32 == 0x40046b05);
CHECK(GPIO_GET_CHIPINFO_IOCTL == 0x4044b401);
#else
CHECK(O_NONBLOCK == 04000 && O_NOCTTY == 0400);
CHECK(SPI_IOC_MESSAGE(1) == 0x40206b00);
CHECK(SPI_IOC_RD_BITS_PER_WORD == 0x80016b03);
CHECK(SPI_IOC_RD_MAX_SPEED_HZ == 0x80046b04);
CHECK(SPI_IOC_RD_MODE32 == 0x80046b05);
CHECK(GPIO_GET_CHIPINFO_IOCTL == 0x8044b401);
#endif

#if defined(__arm__) || defined(__aarch64__)
CHECK(O_NOFOLLOW == 0100000);
#else
CHECK(O_NOFOLLOW == 0400000);
#endif

CHECK(sizeof(struct spi_ioc_transfer) == 32);
FIELD(spi_ioc_transfer, tx_buf, 0);
FIELD(spi_ioc_transfer, rx_buf, 8);
FIELD(spi_ioc_transfer, len, 16);
FIELD(spi_ioc_transfer, speed_hz, 20);
FIELD(spi_ioc_transfer, delay_usecs, 24);
FIELD(spi_ioc_transfer, bits_per_word, 26);
FIELD(spi_ioc_transfer, cs_change, 27);
FIELD(spi_ioc_transfer, tx_nbits, 28);
FIELD(spi_ioc_transfer, rx_nbits, 29);
FIELD(spi_ioc_transfer, pad, 31);

CHECK(sizeof(struct gpiochip_info) == 68);
FIELD(gpiochip_info, lines, 64);

#if defined(GPIO_V2_GET_LINEINFO_IOCTL)
CHECK(GPIO_V2_GET_LINEINFO_IOCTL == 0xc100b405);
CHECK(GPIO_V2_GET_LINE_IOCTL == 0xc250b407);
CHECK(GPIO_V2_LINE_GET_VALUES_IOCTL == 0xc010b40e);
CHECK(GPIO_V2_LINE_FLAG_USED == 1);
CHECK(GPIO_V2_LINE_FLAG_ACTIVE_LOW == 2);
CHECK(GPIO_V2_LINE_FLAG_INPUT == 4);
CHECK(GPIO_V2_LINE_FLAG_OUTPUT == 8);

CHECK(sizeof(struct gpio_v2_line_attribute) == 16);
CHECK(_Alignof(struct gpio_v2_line_attribute) == 8);
FIELD(gpio_v2_line_attribute, flags, 8);
CHECK(sizeof(struct gpio_v2_line_config_attribute) == 24);
FIELD(gpio_v2_line_config_attribute, mask, 16);
CHECK(sizeof(struct gpio_v2_line_config) == 272);
FIELD(gpio_v2_line_config, num_attrs, 8);
FIELD(gpio_v2_line_config, attrs, 32);
CHECK(sizeof(struct gpio_v2_line_info) == 256);
FIELD(gpio_v2_line_info, offset, 64);
FIELD(gpio_v2_line_info, flags, 72);
FIELD(gpio_v2_line_info, attrs, 80);
CHECK(sizeof(struct gpio_v2_line_request) == 592);
FIELD(gpio_v2_line_request, consumer, 256);
FIELD(gpio_v2_line_request, config, 288);
FIELD(gpio_v2_line_request, num_lines, 560);
FIELD(gpio_v2_line_request, fd, 588);
CHECK(sizeof(struct gpio_v2_line_values) == 16);
FIELD(gpio_v2_line_values, mask, 8);
#elif !defined(EXPECT_LEGACY_GPIO_HEADERS)
#error "GPIO v2 UAPI declarations are missing"
#endif
