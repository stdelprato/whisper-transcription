# Whisper Transcription GUI
Whisper Transcription GUI is a powerful and user-friendly application that leverages the OpenAI Whisper model to transcribe audio files. Built with Python and PyQt6, this tool provides an intuitive interface for batch processing audio files, with support for multiple languages and translation options.
Features

🎧 Supports multiple audio formats (.mp3, .wav, .m4a, .flac, .ogg)
🌐 Auto-detection of language and translation to English
🔢 Batch processing of audio files
🎚️ Adjustable temperature settings for transcription accuracy
💻 CPU usage optimization options
📊 Progress tracking and time estimation
📁 Organized output structure mirroring input folders

Requirements

Python 3.8+
PyQt6
transformers
torch
mutagen
psutil

Installation

Clone the repository:
Copygit clone https://github.com/yourusername/whisper-transcription-gui.git
cd whisper-transcription-gui

Create a virtual environment (optional but recommended):
Copypython -m venv venv
source venv/bin/activate  # On Windows, use `venv\Scripts\activate`

Install the required packages:
Copypip install -r requirements.txt


Usage

Run the application:
Copypython main.py

Use the "Explorar" button to select a folder containing audio files.
Choose your language settings:

Select "Español" or "Inglés" for specific languages
Use "Auto-detectar" for automatic language detection
Enable "Traducir a Inglés" for translation to English


Adjust the temperature setting if needed (higher values increase creativity but may reduce accuracy).
Select individual files or use "Transcribir todos" to process all files.
Monitor progress in the output text area and progress bar.
Find transcribed text files in the "results" folder, organized to mirror your input folder structure.

Contributing
Contributions are welcome! Please feel free to submit a Pull Request.
License
This project is licensed under the MIT License - see the LICENSE file for details.

Built with ❤️ using OpenAI's Whisper model and PyQt6