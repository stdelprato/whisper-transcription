import os
import time
import torch
from faster_whisper import WhisperModel
from PyQt6.QtCore import QThread, pyqtSignal
from utils.audio_utils import get_audio_duration, format_duration

class TranscriptionThread(QThread):
    transcription_done = pyqtSignal(str, float, float)
    all_transcriptions_done = pyqtSignal()
    progress_update = pyqtSignal(int, int)

    def __init__(self, pipe, files, directory, language, translate, transcription_options, auto_detect, base_output_dir, model_type):
        super().__init__()
        self.pipe = pipe
        self.files = files
        self.directory = directory
        self.language = language
        self.translate = translate
        self.transcription_options = transcription_options
        self.auto_detect = auto_detect
        self.base_output_dir = base_output_dir
        self.model_type = model_type

    def run(self):
        total_files = len(self.files)
        for index, audio_file in enumerate(self.files, 1):
            if ' > ' in audio_file:
                subfolder, filename = audio_file.split(' > ')
                input_path = os.path.join(self.directory, subfolder, filename)
                output_path = os.path.join(self.base_output_dir, subfolder, f"{os.path.splitext(filename)[0]}.txt")
            else:
                input_path = os.path.join(self.directory, audio_file)
                output_path = os.path.join(self.base_output_dir, f"{os.path.splitext(audio_file)[0]}.txt")
            
            self.transcribe_audio(input_path, output_path)
            self.progress_update.emit(index, total_files)
        
        self.all_transcriptions_done.emit()

    def transcribe_audio(self, input_path, output_path):
        start_time = time.time()
        
        corrected_input_path = os.path.normpath(input_path)
        
        if not os.path.exists(corrected_input_path):
            raise FileNotFoundError(f"No se pudo encontrar el archivo: {corrected_input_path}")
        
        audio_duration = get_audio_duration(corrected_input_path)
        
        # Configurar parámetros de transcripción
        task = "translate" if self.translate else "transcribe"
        language = self.language if not self.auto_detect else None
        
        if isinstance(self.pipe, WhisperModel):
            print("Transcribiendo con Faster-Whisper")
            segments, info = self.pipe.transcribe(
                corrected_input_path,
                task=task,
                language=language,
                temperature=self.transcription_options.get('temperature', 0.0),
                beam_size=5,
                patience=1.2,
                vad_filter=True,
                vad_parameters=dict(min_silence_duration_ms=500)
            )
            
            with open(output_path, 'w', encoding='utf-8') as f:
                f.write("Transcripción con timestamps:\n")
                for segment in segments:
                    f.write(f"[{self.format_timestamp(segment.start)} -> {self.format_timestamp(segment.end)}] {segment.text}\n")
        else:
            print("Transcribiendo con Whisper Original")
            generate_kwargs = {
                "task": task,
                "language": language,
                "max_new_tokens": 256,
                "temperature": self.transcription_options.get('temperature', 0.0),
                "do_sample": False,
                "num_beams": 1
            }

            result = self.pipe(corrected_input_path, return_timestamps=True, generate_kwargs=generate_kwargs)
            
            with open(output_path, 'w', encoding='utf-8') as f:
                f.write("Transcripción con timestamps:\n")
                if "chunks" in result:
                    for chunk in result["chunks"]:
                        start = chunk.get('timestamp', [0, 0])[0]
                        end = chunk.get('timestamp', [0, 0])[1]
                        chunk_text = chunk.get('text', '')
                        f.write(f"[{self.format_timestamp(start)} -> {self.format_timestamp(end)}] {chunk_text}\n")
                else:
                    import json
                    f.write(json.dumps(result, indent=2))
        
        end_time = time.time()
        transcription_time = end_time - start_time
        self.transcription_done.emit(input_path, transcription_time, audio_duration)

    def format_timestamp(self, seconds):
        return format_duration(seconds)

    def format_text(self, text):
        import re
        if not text.endswith('.'):
            text += '.'
        sentences = re.split('(?<=[.!?]) +', text)
        sentences = [s.capitalize() for s in sentences]
        paragraphs = []
        for i in range(0, len(sentences), 4):
            paragraph = ' '.join(sentences[i:i+4])
            paragraphs.append(paragraph)
        formatted_text = '\n\n'.join(paragraphs)
        return formatted_text