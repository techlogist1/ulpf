# Static build of the ulpf binary. Alpine's Rust toolchain targets musl, so binaries are
# statically linked by default; build-base provides the C compiler rusqlite's bundled
# SQLite needs. The runtime image is `scratch`: one executable plus the plain-text
# parser and mapping folders, nothing else. No network access is needed at runtime.
FROM rust:1.95-alpine AS build
RUN apk add --no-cache build-base file
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p ulpf --locked \
 && file target/release/ulpf \
 && file target/release/ulpf | grep -Eq 'static(-pie)? linked|statically linked' \
 && strip target/release/ulpf

FROM scratch
COPY --from=build /src/target/release/ulpf /ulpf
COPY parsers /parsers
COPY mappings /mappings
WORKDIR /data
ENTRYPOINT ["/ulpf"]
CMD ["--help"]
