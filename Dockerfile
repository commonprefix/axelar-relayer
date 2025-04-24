# Builder Stage
FROM rust:1.86-bookworm AS builder

# Set the working directory
WORKDIR /app

# Copy Cargo manifest files first to cache dependencies
COPY Cargo.toml Cargo.lock ./

# Create a dummy source file to compile dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies
RUN cargo build --release

# Now copy the entire project source code into the container
COPY src/ ./src/

# Build all binaries in src/bin; this will build every binary.
RUN cargo build --release --bins

# Final Stage: Produce a lean runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && apt-get install -y libssl3 && apt-get install -y tzdata

WORKDIR /app

COPY config.*.yaml ./

COPY certs/ ./certs/

# Build argument to decide which binary to include in this image.
ARG BINARY_NAME
RUN if [ -z "$BINARY_NAME" ]; then \
      echo >&2 "ERROR: you must set BINARY_NAME env"; \
      exit 1; \
    fi
ENV BINARY_NAME=${BINARY_NAME}

# Copy the desired binary from the builder stage into the final image
COPY --from=builder /app/target/release/${BINARY_NAME} /usr/local/bin/${BINARY_NAME}

# Run the selected binary
CMD exec /usr/local/bin/$BINARY_NAME
