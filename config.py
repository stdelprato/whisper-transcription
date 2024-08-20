import os

APP_TITLE = "Whisper Transcription 0.5.5"
MODEL_ID = "large-v3"
SUPPORTED_AUDIO_FORMATS = ('.mp3', '.wav', '.m4a', '.flac', '.ogg')

# Definir MODEL_DIR en un lugar específico de tu elección
MODEL_DIR = os.path.join(os.path.expanduser("~"), "whisper_models")