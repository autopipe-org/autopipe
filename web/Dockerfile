FROM node:20-slim AS builder

WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci

COPY . .
RUN npm run build

FROM node:20-slim
RUN groupadd -r autopipe && useradd -r -g autopipe -s /bin/false autopipe
WORKDIR /app
COPY --from=builder /app/build ./build
COPY --from=builder /app/package.json ./
COPY --from=builder /app/node_modules ./node_modules
RUN chown -R autopipe:autopipe /app

USER autopipe
EXPOSE 8090
ENV PORT=8090
CMD ["node", "build"]
