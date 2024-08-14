from pydub import AudioSegment
import os
from config import SUPPORTED_AUDIO_FORMATS

def is_audio_file(filename):
    return filename.lower().endswith(SUPPORTED_AUDIO_FORMATS)

def get_audio_duration(file_path):
    try:
        audio = AudioSegment.from_file(file_path)
        duration_seconds = len(audio) / 1000.0
        return duration_seconds
    except Exception as e:
        print(f"Error al obtener la duración de {file_path}: {str(e)}")
        return 0

def format_duration(seconds):
    minutes, seconds = divmod(int(seconds), 60)
    hours, minutes = divmod(minutes, 60)
    return f"{hours:02d}:{minutes:02d}:{seconds:02d}"