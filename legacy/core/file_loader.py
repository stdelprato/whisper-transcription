import os
from PyQt6.QtCore import QThread, pyqtSignal
from utils.audio_utils import is_audio_file, get_audio_duration

class LoadFilesThread(QThread):
    file_found = pyqtSignal(str, str)
    
    def __init__(self, directory):
        super().__init__()
        self.directory = directory
    
    def run(self):
        for root, dirs, files in os.walk(self.directory):
            if self.isInterruptionRequested():
                return
            for file in files:
                if self.isInterruptionRequested():
                    return
                if is_audio_file(file):
                    file_path = os.path.join(root, file)
                    relative_path = os.path.relpath(root, self.directory)
                    self.file_found.emit(file_path, relative_path)