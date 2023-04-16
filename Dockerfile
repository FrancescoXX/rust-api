# Build stage
FROM rust:1.67 as builder

WORKDIR /app

ARG DATABASE_URL
ENV DATABASE_URL=$DATABASE_URL

COPY . .

# Run cargo clean to remove any cached build artifacts
RUN cargo clean

# Build the project
RUN cargo build --release

# Production stage
FROM debian:buster-slim

WORKDIR /usr/local/bin

COPY --from=builder /app/target/release/rust_postgresql_tutorial .

CMD ["./rust_postgresql_tutorial"]