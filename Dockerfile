# Builder Stage
FROM rust:1.86-bookworm AS builder

# Build argument to decide which binary to include in this image
ARG BINARY_NAME
RUN if [ -z "$BINARY_NAME" ]; then \
      echo >&2 "ERROR: you must set BINARY_NAME env"; \
      exit 1; \
    fi

# Set the working directory
WORKDIR /app

# Copy workspace Cargo files first
COPY Cargo.toml Cargo.lock ./
COPY Cargo.lock ./

# Copy member Cargo files
COPY relayer_base/Cargo.toml ./relayer_base/
COPY recovery_tools/Cargo.toml ./recovery_tools/
COPY xrpl/Cargo.toml ./xrpl/

# Create dummy files for each workspace member to cache dependencies
RUN mkdir -p relayer_base/src/bin recovery_tools/src/bin xrpl/src/bin/recovery && \
    echo 'fn main() {}' > recovery_tools/src/bin/proof_retrier.rs && \
    echo 'fn main() {}' > recovery_tools/src/bin/dlq_recovery.rs && \
    echo 'fn main() {}' > relayer_base/src/bin/price_feed.rs && \
    echo 'fn main() {}' > xrpl/src/bin/xrpl_ingestor.rs && \
    echo 'fn main() {}' > xrpl/src/bin/xrpl_distributor.rs && \
    echo 'fn main() {}' > xrpl/src/bin/xrpl_subscriber.rs && \
    echo 'fn main() {}' > xrpl/src/bin/xrpl_includer.rs && \
    echo 'fn main() {}' > xrpl/src/bin/xrpl_funder.rs && \
    echo 'fn main() {}' > xrpl/src/bin/xrpl_ticket_creator.rs && \
    echo 'fn main() {}' > xrpl/src/bin/xrpl_ticket_monitor.rs && \
    echo 'fn main() {}' > xrpl/src/bin/recovery/xrpl_subscriber_recovery.rs && \
    echo 'fn main() {}' > xrpl/src/bin/recovery/xrpl_task_recovery.rs && \
    echo 'fn main() {}' > xrpl/src/bin/xrpl_heartbeat_monitor.rs

# Build dependencies (this will cache them)
RUN cargo build --release

# Remove the dummy files
RUN rm -rf recovery_tools/src relayer_base/src xrpl/src

# Now copy the actual source code
COPY relayer_base/src/ ./relayer_base/src/
COPY recovery_tools/src/ ./recovery_tools/src/
COPY xrpl/src/ ./xrpl/src/

# Build the project with actual source code
RUN if [ "${BINARY_NAME}" = "proof_retrier" ]; then \
      cargo build --release --package recovery-tools --bin ${BINARY_NAME}; \
    elif [ "${BINARY_NAME}" = "price_feed" ]; then \
      cargo build --release --package relayer-base --bin ${BINARY_NAME}; \
    else \
      cargo build --release --package xrpl --bin ${BINARY_NAME}; \
    fi

# Final Stage: Produce a lean runtime image
FROM debian:bookworm-slim

# Install runtime dependencies and clean up in one layer
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    tzdata && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Set the base path environment variable
ENV BASE_PATH=/app

# Copy config and certs
COPY certs/ ./certs/
COPY config/ ./config/

# Build argument to decide which binary to include in this image
ARG BINARY_NAME
RUN if [ -z "$BINARY_NAME" ]; then \
      echo >&2 "ERROR: you must set BINARY_NAME env"; \
      exit 1; \
    fi
ENV BINARY_NAME=${BINARY_NAME}

# Copy the desired binary from the builder stage
COPY --from=builder /app/target/release/${BINARY_NAME} /usr/local/bin/${BINARY_NAME}

# Run the selected binary
ENTRYPOINT /usr/local/bin/$BINARY_NAME