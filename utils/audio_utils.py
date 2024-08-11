from mutagen import File
import config

def is_audio_file(filename):
    return filename.lower().endswith(config.SUPPORTED_AUDIO_FORMATS)

def get_audio_duration(file_path):
    audio = File(file_path)
    if audio is not None:
        return audio.info.length
    return 0

def format_duration(seconds):
    minutes, seconds = divmod(int(seconds), 60)
    hours, minutes = divmod(minutes, 60)
    return f"{hours:02d}:{minutes:02d}:{seconds:02d}"