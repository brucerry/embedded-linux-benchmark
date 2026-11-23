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

#include <netinet/in.h>
#include <poll.h>
#include <sys/socket.h>

CHECK(AF_INET == 2);
CHECK(sizeof(struct sockaddr_in) == 16);
FIELD(sockaddr_in, sin_port, 2);
FIELD(sockaddr_in, sin_addr, 4);
CHECK(sizeof(struct pollfd) == 8);
CHECK(POLLIN == 1 && POLLOUT == 4);
CHECK(sizeof(nfds_t) == sizeof(unsigned long));
#if defined(__mips__)
CHECK(SOCK_STREAM == 2 && SOCK_NONBLOCK == 0x80 && EINPROGRESS == 150);
#else
CHECK(SOCK_STREAM == 1 && SOCK_NONBLOCK == 04000 && EINPROGRESS == 115);
#endif
CHECK(SOCK_CLOEXEC == 02000000);

#include <linux/videodev2.h>
#include <sys/mman.h>

CHECK(V4L2_CAP_VIDEO_CAPTURE == 1);
CHECK(V4L2_CAP_VIDEO_CAPTURE_MPLANE == 0x1000);
CHECK(V4L2_CAP_STREAMING == 0x04000000);
CHECK(V4L2_CAP_DEVICE_CAPS == 0x80000000);
CHECK(V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE == 9);
CHECK(V4L2_FIELD_ALTERNATE == 7);
CHECK(V4L2_CAP_TIMEPERFRAME == 0x1000);
CHECK(V4L2_BUF_FLAG_ERROR == 0x40);
CHECK(V4L2_MEMORY_MMAP == 1);
CHECK(PROT_READ == 1 && PROT_WRITE == 2 && MAP_SHARED == 1);
FIELD(v4l2_pix_format, sizeimage, 20);
CHECK(sizeof(struct v4l2_plane_pix_format) == 20);
FIELD(v4l2_pix_format_mplane, num_planes, 180);
FIELD(v4l2_captureparm, timeperframe, 8);
#if __SIZEOF_POINTER__ == 8
CHECK(sizeof(off_t) == 8);
CHECK(sizeof(struct v4l2_capability) == 104);
CHECK(sizeof(struct v4l2_format) == 208);
FIELD(v4l2_format, fmt, 8);
CHECK(sizeof(struct v4l2_streamparm) == 204);
CHECK(sizeof(struct v4l2_requestbuffers) == 20);
CHECK(sizeof(struct v4l2_buffer) == 88);
FIELD(v4l2_buffer, timestamp, 24);
FIELD(v4l2_buffer, sequence, 56);
FIELD(v4l2_buffer, m, 64);
CHECK(sizeof(struct v4l2_plane) == 64);
FIELD(v4l2_plane, m, 8);
CHECK(VIDIOC_QUERYCAP == 0x80685600);
CHECK(VIDIOC_G_FMT == 0xc0d05604);
CHECK(VIDIOC_REQBUFS == 0xc0145608);
CHECK(VIDIOC_QUERYBUF == 0xc0585609);
CHECK(VIDIOC_QBUF == 0xc058560f);
CHECK(VIDIOC_DQBUF == 0xc0585611);
CHECK(VIDIOC_STREAMON == 0x40045612);
CHECK(VIDIOC_STREAMOFF == 0x40045613);
CHECK(VIDIOC_G_PARM == 0xc0cc5615);
#endif

#include <linux/can.h>
#include <linux/can/raw.h>
#include <net/if.h>
#include <linux/if_arp.h>

CHECK(AF_CAN == 29 && SOCK_RAW == 3);
CHECK(IF_NAMESIZE == 16 && ARPHRD_CAN == 280);
CHECK(CAN_RAW == 1 && SOL_CAN_RAW == 101);
CHECK(CAN_RAW_FILTER == 1 && CAN_RAW_LOOPBACK == 3 && CAN_RAW_FD_FRAMES == 5);
CHECK((CAN_EFF_FLAG | CAN_RTR_FLAG | CAN_SFF_MASK) == 0xc00007ff);
CHECK(sizeof(struct sockaddr_can) == 24);
FIELD(sockaddr_can, can_ifindex, 4);
FIELD(sockaddr_can, can_addr, 8);
CHECK(sizeof(struct can_filter) == 8);
CHECK(CAN_MTU == 16 && CANFD_MTU == 72);
FIELD(can_frame, can_dlc, 4);
FIELD(can_frame, data, 8);
FIELD(canfd_frame, len, 4);
FIELD(canfd_frame, data, 8);
