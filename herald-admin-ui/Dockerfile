FROM node:22-alpine
WORKDIR /app

COPY package.json package-lock.json* ./
RUN npm install && npm install serve

COPY . .
RUN npm run build

COPY docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh

ENV HERALD_URL=http://localhost:6201
ENV PORT=8080

EXPOSE 8080

ENTRYPOINT ["/docker-entrypoint.sh"]
