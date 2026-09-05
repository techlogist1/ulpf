# Static build of the ulpf binary. Alpine's Rust toolchain targets musl, so binaries are
# statically linked by default; build-base provides the C compiler rusqlite's bundled
# SQLite needs. The UI is three prebuilt files under ui/dist embedded at compile time, so
# the image needs no node. The runtime image is `scratch`: one executable plus the
# plain-text parser and mapping folders, nothing else. No network access is needed at
# runtime; `serve` listens where told and never connects out. The image is built with
# --profile dist (fat LTO, one codegen unit) -- the profile every shipped number is
# measured on; the default release profile drops LTO so a tester's build is quick.
FROM rust:1.95-alpine AS build
RUN apk add --no-cache build-base file
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY ui/dist ./ui/dist
RUN cargo build --profile dist -p ulpf --locked \
 && file target/dist/ulpf \
 && file target/dist/ulpf | grep -Eq 'static(-pie)? linked|statically linked' \
 && strip target/dist/ulpf

FROM scratch
COPY --from=build /src/target/dist/ulpf /ulpf
COPY parsers /parsers
COPY mappings /mappings
WORKDIR /
# serve binds 127.0.0.1 by default; inside a container pass --listen 0.0.0.0:7878
EXPOSE 7878
ENTRYPOINT ["/ulpf"]
CMD ["--help"]
