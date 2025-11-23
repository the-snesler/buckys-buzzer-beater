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

# Stage 2: Setup the Bun server
FROM oven/bun:1 AS server

WORKDIR /app

# Copy server files
COPY apps/node_server/package.json apps/node_server/bun.lock ./
RUN bun install --frozen-lockfile

COPY apps/node_server/src ./src

# Copy built web assets from previous stage
COPY --from=web-builder /app/dist ./public

# Set environment variables
ENV PORT=3000
ENV PUBLIC_DIR=/app/public

EXPOSE 3000

CMD ["bun", "run", "src/index.ts"]
