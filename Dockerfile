# Stage 1: Build the web app
FROM node:20-alpine AS web-builder

WORKDIR /app

# Install pnpm
RUN corepack enable && corepack prepare pnpm@latest --activate

# Copy web app files
COPY apps/web/package.json apps/web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY apps/web/ ./
RUN pnpm build

# Stage 2: Build the Rust server
FROM rust:alpine AS rust-builder

WORKDIR /app

# Install musl-dev for static linking
RUN apk add --no-cache musl-dev

# Copy server files
COPY apps/server/ ./

# Build release binary
RUN cargo build --release

# Stage 3: Runtime
FROM alpine:latest

WORKDIR /app

# Copy built binary from rust-builder
COPY --from=rust-builder /app/target/release/madhacks2025 ./server

# Copy built web assets from web-builder
COPY --from=web-builder /app/dist ./public

# Set environment variables
ENV PORT=3000

EXPOSE 3000

CMD ["./server"]
