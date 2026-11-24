FROM python:3.12-slim-bookworm
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 HOME=/tmp
WORKDIR /app
RUN pip install --no-cache-dir "pipecat-ai[livekit,deepgram,cartesia,silero]==1.17.0" "livekit-api==1.2.3"
COPY app.py voice.py session.py index.html app.js /app/
USER 10001:10001
EXPOSE 8080
CMD ["python","/app/app.py"]
