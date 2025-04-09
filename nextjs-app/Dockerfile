# Use an official Node.js runtime as a parent image
# Choose a version compatible with your project (Node 18+ required for Next.js 15)
FROM node:18-alpine

# Set the working directory in the container
WORKDIR /app

# Copy package.json and package-lock.json (or yarn.lock)
COPY package*.json ./

# Install app dependencies
RUN npm install

# Bundle app source inside Docker image
COPY . .

# Expose port 3000 to the Docker network
EXPOSE 3000

# Define the command to run your app using CMD which defines the runtime command
# This will be overridden by the command in docker-compose.yml for development
CMD ["npm", "run", "start"] # Default command for production builds
