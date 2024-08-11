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

# Clone the repository
$ git clone https://github.com/yourusername/whisper-transcription-gui.git
$ cd whisper-transcription-gui

# Create and activate a virtual environment (optional but recommended)
$ python -m venv venv
$ source venv/bin/activate  # On Windows, use `venv\Scripts\activate`

# Install the required packages
$ pip install -r requirements.txt


Usage

# Run the application
$ python main.py

# The GUI will now open. Follow these steps:
# 1. Click "Explorar" to select a folder with audio files
# 2. Choose language settings
# 3. Adjust temperature if needed
# 4. Click "Transcribir todos" or select specific files
# 5. Wait for transcription to complete
# 6. Find output in the "results" folder

Contributing
Contributions are welcome! Please feel free to submit a Pull Request.
License
This project is licensed under the MIT License - see the LICENSE file for details.

Built with ❤️ using OpenAI's Whisper model and PyQt6