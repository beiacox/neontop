# NeonTop FPS: capa Vulkan y librería de OpenGL, en 64 y 32 bits (juegos de 32 bits / Wine).
# Flags propios (no los de makepkg): el estado compartido entre ambas librerías necesita la
# interposición de símbolos normal de ELF (sin -fno-semantic-interposition ni -Bsymbolic).
CC ?= cc
FLAGS = -O2 -fPIC -shared -Wall -Wextra -fvisibility=hidden -pthread
SRC_VK = vulkan_layer.c common.c
SRC_GL = gl_preload.c common.c

all: build/64/libneontop_layer.so build/64/libneontop_gl.so build/32/libneontop_layer.so build/32/libneontop_gl.so

build/64/libneontop_layer.so: $(SRC_VK) neontop_fps.h
	mkdir -p build/64 && $(CC) $(FLAGS) -m64 -o $@ $(SRC_VK)
build/64/libneontop_gl.so: $(SRC_GL) neontop_fps.h
	mkdir -p build/64 && $(CC) $(FLAGS) -m64 -o $@ $(SRC_GL) -ldl
build/32/libneontop_layer.so: $(SRC_VK) neontop_fps.h
	mkdir -p build/32 && $(CC) $(FLAGS) -m32 -o $@ $(SRC_VK)
build/32/libneontop_gl.so: $(SRC_GL) neontop_fps.h
	mkdir -p build/32 && $(CC) $(FLAGS) -m32 -o $@ $(SRC_GL) -ldl

clean:
	rm -rf build
.PHONY: all clean
