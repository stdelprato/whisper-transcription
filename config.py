import os

APP_TITLE = "Whisper Transcription 0.6"
MODEL_ID = "openai/whisper-large-v2"
SUPPORTED_AUDIO_FORMATS = ('.mp3', '.wav', '.m4a', '.flac', '.ogg')

# Define MODEL_DIR in a specific location of your choice
MODEL_DIR = os.path.join(os.path.expanduser("~"), "whisper_models")