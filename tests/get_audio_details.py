import os
import mutagen
from mutagen import File
from pydub import AudioSegment

def get_audio_files(directory):
    audio_files = []
    for root, _, files in os.walk(directory):
        for file in files:
            if file.lower().endswith(('.mp3', '.wav', '.m4a', '.flac', '.ogg')):
                audio_files.append(os.path.join(root, file))
    return audio_files

def get_audio_duration_mutagen(file_path):
    print(f"Intentando obtener la duración con Mutagen de: {file_path}")
    try:
        audio = File(file_path)
        if audio is not None:
            duration = audio.info.length
            print(f"Duración obtenida con Mutagen: {duration} segundos")
            return duration
        else:
            print(f"No se pudo leer el archivo de audio con Mutagen: {file_path}")
    except Exception as e:
        print(f"Error al obtener la duración con Mutagen de {file_path}: {str(e)}")
    return 0

def get_audio_duration_pydub(file_path):
    print(f"Intentando obtener la duración con pydub de: {file_path}")
    try:
        audio = AudioSegment.from_file(file_path)
        duration_seconds = len(audio) / 1000.0
        print(f"Duración obtenida con pydub: {duration_seconds} segundos")
        return duration_seconds
    except Exception as e:
        print(f"Error al obtener la duración con pydub de {file_path}: {str(e)}")
        return 0

def get_detailed_audio_info(file_path):
    print(f"\nAnalizando archivo: {file_path}")
    print(f"Tamaño del archivo: {os.path.getsize(file_path)} bytes")
    
    try:
        audio = File(file_path)
        print(f"Tipo de archivo detectado por Mutagen: {type(audio)}")
        
        if audio is not None:
            print("Información del archivo:")
            for key, value in audio.info.__dict__.items():
                print(f"  {key}: {value}")
        else:
            print("No se pudo leer el archivo de audio con Mutagen.")
    except Exception as e:
        print(f"Error al analizar el archivo con Mutagen: {str(e)}")

def read_file_start(file_path, bytes_to_read=1024):
    with open(file_path, 'rb') as f:
        data = f.read(bytes_to_read)
    print(f"\nPrimeros {bytes_to_read} bytes del archivo:")
    print(data.hex())

def main():
    print(f"Versión de Mutagen: {mutagen.__version__}")
    
    directory = input("Ingrese la ruta de la carpeta con archivos de audio: ")
    audio_files = get_audio_files(directory)
    
    if not audio_files:
        print("No se encontraron archivos de audio en la carpeta especificada.")
        return
    
    print("\nArchivos de audio encontrados:")
    for i, file in enumerate(audio_files, 1):
        print(f"{i}. {os.path.basename(file)}")
    
    choice = int(input("\nSeleccione el número del archivo que desea analizar: ")) - 1
    if choice < 0 or choice >= len(audio_files):
        print("Selección inválida.")
        return
    
    selected_file = audio_files[choice]
    
    get_audio_duration_mutagen(selected_file)
    get_audio_duration_pydub(selected_file)
    get_detailed_audio_info(selected_file)
    read_file_start(selected_file)

if __name__ == "__main__":
    main()