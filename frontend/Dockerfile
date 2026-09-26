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
COPY --from=deps /app/node_modules ./node_modules
COPY --from=build /app/dist ./dist
COPY package.json ./
COPY .env.example ./  # 运行时密钥经环境变量/挂载注入, 模板进镜像

EXPOSE 3000
CMD ["npm", "start"]
