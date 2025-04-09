# backend/Dockerfile
# Use Python 3.10 slim image (3.9+ recommended)
FROM python:3.10-slim

WORKDIR /app

# Install system dependencies if needed (e.g., for certain DB drivers)
# RUN apt-get update && apt-get install -y --no-install-recommends some-package && rm -rf /var/lib/apt/lists/*

# Copy requirements first to leverage Docker cache
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy the rest of the application code
COPY . .

# Expose the port defined in the .env file (will be mapped by docker-compose)
# EXPOSE ${FASTAPI_PORT:-8001} # EXPOSE is informational, mapping happens in compose

# The command to run the application will be specified in docker-compose.yml
# Example (uvicorn for FastAPI): CMD ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8001"]
