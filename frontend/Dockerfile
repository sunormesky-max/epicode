FROM node:20-alpine AS base
WORKDIR /app

FROM base AS deps
COPY package.json package-lock.json ./
# 代理配置通过 build arg 注入，不硬编码内网 IP，不禁用 SSL 验证
ARG HTTP_PROXY=""
ARG HTTPS_PROXY=""
ARG NPM_REGISTRY="https://registry.npmjs.org"
ENV HTTP_PROXY=${HTTP_PROXY}
ENV HTTPS_PROXY=${HTTPS_PROXY}
RUN npm config set registry ${NPM_REGISTRY} \
    && npm config set fund false \
    && npm config set audit false
RUN --mount=type=cache,target=/root/.npm \
    npm ci --prefer-offline --no-audit

FROM deps AS build
COPY . .
RUN npm run build

FROM node:20-alpine AS production
WORKDIR /app
COPY --from=build --chown=node:node /app/dist/public ./dist
COPY --chown=node:node package.json ./package.json
COPY --chown=node:node server.mjs ./server.mjs
ENV NODE_ENV=production HOST=0.0.0.0 PORT=3000
USER 1000:1000

EXPOSE 3000
CMD ["npm", "start"]
