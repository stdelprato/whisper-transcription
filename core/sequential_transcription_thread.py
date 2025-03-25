import os
import subprocess
import re
from PyQt6.QtCore import QThread, pyqtSignal
from utils.audio_utils import get_audio_duration

def detect_language(text):
    try:
        from langdetect import detect
        return detect(text)
    except Exception:
        if any(char in text for char in "áéíóúñ"):
            return "es"
        return "en"

class SequentialTranscriptionThread(QThread):
    transcription_started = pyqtSignal(str)
    transcription_finished = pyqtSignal(str, bool)
    output_received = pyqtSignal(str)
    progress_update = pyqtSignal(str, str, str)

    def __init__(self, files_to_transcribe, executable_path, base_output_dir, folder_input, current_language, translate):
        super().__init__()
        self.files_to_transcribe = files_to_transcribe
        self.executable_path = executable_path
        self.base_output_dir = base_output_dir
        self.folder_input = folder_input
        self.current_language = current_language
        self.translate = translate

    def run(self):
        for audio_file in self.files_to_transcribe:
            self.transcription_started.emit(audio_file)
            if ' > ' in audio_file:
                subfolder, filename = audio_file.split(' > ')
                input_path = os.path.join(self.folder_input, subfolder, filename)
                output_subfolder = os.path.join(self.base_output_dir, subfolder)
            else:
                input_path = os.path.join(self.folder_input, audio_file)
                output_subfolder = self.base_output_dir

            os.makedirs(output_subfolder, exist_ok=True)
            total_duration = get_audio_duration(input_path)

            candidate_configs = [
                {"language": "en", "temperature": 0.00001},
                {"language": "English", "temperature": 0.00001},
                {"language": "en", "temperature": 0.0},
                {"language": "en", "temperature": 0.1},
                {"language": "en", "temperature": 0.2},
                {"language": "en", "temperature": 1},
            ]

            task_option = "translate" if self.translate else "transcribe"
            successful = False

            for config in candidate_configs:
                current_language = config["language"]
                temperature_value = config["temperature"]
                language_option = f"-l {current_language}" if current_language else ""
                command = (
                    f'"{self.executable_path}" "{input_path}" {language_option} -m large-v2 '
                    f'--temperature {temperature_value} --compression_ratio_threshold 2 '
                    f'--task {task_option} --sentence --output_dir "{output_subfolder}" --output_format txt'
                )

                process = subprocess.Popen(command, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                           text=True, bufsize=1, universal_newlines=True)
                output_buffer = []
                checked = False
                spanish_detected = False

                while True:
                    line = process.stdout.readline()
                    if not line:
                        break
                    line = line.strip()
                    self.output_received.emit(line)
                    output_buffer.append(line)

                    match = re.search(r'\[(\d{2}:\d{2}\.\d{3}) -->', line)
                    if match:
                        start = match.group(1)
                        if ':' in start:
                            try:
                                minutes, seconds = start.split(':')
                                current_time = int(minutes) * 60 + float(seconds)
                            except Exception:
                                current_time = 0.0
                        else:
                            current_time = 0.0

                        progress_percent = (current_time / total_duration) * 100 if total_duration > 0 else 0
                        self.progress_update.emit(start, "progress", audio_file)

                        if progress_percent >= 15 and not checked:
                            detected_lang = detect_language(" ".join(output_buffer))
                            if detected_lang == "es":
                                spanish_detected = True
                                break
                            checked = True

                process.terminate()
                process.wait()

                if not spanish_detected:
                    successful = True
                    break

            self.transcription_finished.emit(audio_file, successful)