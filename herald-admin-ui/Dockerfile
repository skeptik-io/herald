FROM node:22-alpine
WORKDIR /app

COPY package.json package-lock.json* ./
RUN npm install

COPY . .
RUN npm run build

ENV HERALD_URL=http://localhost:6201
ENV PORT=8080

EXPOSE 8080

CMD ["node", "server.js"]
