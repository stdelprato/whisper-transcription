import tkinter as tk
from tkinter import filedialog, messagebox
import os
import re

class TimestampNotepad(tk.Tk):
    def __init__(self, file_path=None):
        super().__init__()
        self.title("Timestamp Notepad")
        self.geometry("800x600")
        
        self.frame = tk.Frame(self, bg="#FFFAF0")
        self.frame.pack(fill=tk.BOTH, expand=True)
        
        self.text_widget = tk.Text(self.frame, wrap=tk.WORD, font=('Consolas', 11), bg="#FFFAF0")
        self.text_widget.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        
        self.scrollbar = tk.Scrollbar(self.frame, command=self.text_widget.yview)
        self.scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        
        self.text_widget.config(yscrollcommand=self.scrollbar.set)
        
        self.menu_bar = tk.Menu(self)
        self.file_menu = tk.Menu(self.menu_bar, tearoff=0)
        self.file_menu.add_command(label="Abrir", command=self.open_file)
        self.file_menu.add_command(label="Guardar", command=self.save_file)
        self.menu_bar.add_cascade(label="Archivo", menu=self.file_menu)
        
        # Agregar menú de opciones
        self.options_menu = tk.Menu(self.menu_bar, tearoff=0)
        self.text_locked = tk.BooleanVar(value=True)
        self.options_menu.add_checkbutton(label="Bloquear texto", variable=self.text_locked, command=self.toggle_text_lock)
        self.menu_bar.add_cascade(label="Opciones", menu=self.options_menu)
        
        self.config(menu=self.menu_bar)
        
        self.text_widget.bind("<<Copy>>", self.custom_copy)
        self.text_widget.bind("<Configure>", self.on_text_configure)
        
        self.text_widget.tag_configure("timestamp", lmargin1=0, lmargin2=0, selectbackground="#FFFAF0", selectforeground="#444444")
        self.text_widget.tag_configure("content", lmargin1=220, lmargin2=220)
        
        # Aplicar el bloqueo de texto inicial
        self.toggle_text_lock()
        
        # Agregar binding para Ctrl+Z
        self.text_widget.bind("<Control-z>", self.custom_undo)

        # Agregar menú contextual
        self.context_menu = tk.Menu(self, tearoff=0)
        self.context_menu.add_command(label="Copiar", command=self.context_copy)
        self.context_menu.add_command(label="Pegar", command=self.context_paste)
        self.text_widget.bind("<Button-3>", self.show_context_menu)
        
        if file_path:
            self.open_file(file_path)
    
    def toggle_text_lock(self):
        if self.text_locked.get():
            self.text_widget.config(state=tk.DISABLED)
        else:
            self.text_widget.config(state=tk.NORMAL)

    def custom_undo(self, event):
        if not self.text_locked.get():
            return "break"  # Esto previene que se realice la acción de deshacer
    
    def show_context_menu(self, event):
        self.context_menu.tk_popup(event.x_root, event.y_root)
    
    def context_copy(self):
        self.text_widget.event_generate("<<Copy>>")
    
    def context_paste(self):
        if not self.text_locked.get():
            self.text_widget.event_generate("<<Paste>>")
    
    def on_text_configure(self, event):
        self.after(10, self.update_layout)
        
    def update_layout(self):
        content = self.text_widget.get("1.0", tk.END)
        self.text_widget.delete("1.0", tk.END)
        lines = content.split('\n')
        for line in lines:
            match = re.match(r'(\[.*?\])(.*)', line)
            if match:
                timestamp, text = match.groups()
                self.text_widget.insert(tk.END, f"{timestamp.ljust(20)}", "timestamp")
                self.text_widget.insert(tk.END, f"{text}\n", "content")
            else:
                self.text_widget.insert(tk.END, " " * 20, "timestamp")
                self.text_widget.insert(tk.END, f"{line}\n", "content")
        
    def open_file(self, file_path=None):
        if file_path is None:
            file_path = filedialog.askopenfilename(filetypes=[("Archivos de texto", "*.txt")])
        
        if file_path and os.path.isfile(file_path):
            with open(file_path, 'r', encoding='utf-8') as file:
                content = file.read()
                self.text_widget.config(state=tk.NORMAL)  # Temporalmente habilitar edición
                self.text_widget.delete("1.0", tk.END)
                self.text_widget.insert(tk.END, content)
                self.update_layout()
                self.toggle_text_lock()  # Volver a aplicar el estado de bloqueo
            self.title(f"Timestamp Notepad - {os.path.basename(file_path)}")
        else:
            print(f"Archivo no encontrado o no válido: {file_path}")
    
    def save_file(self):
        file_path = filedialog.asksaveasfilename(defaultextension=".txt", filetypes=[("Archivos de texto", "*.txt")])
        if file_path:
            content = self.text_widget.get("1.0", tk.END)
            with open(file_path, 'w', encoding='utf-8') as file:
                file.write(content)
            messagebox.showinfo("Guardado", "Archivo guardado exitosamente.")
    
    def custom_copy(self, event):
        try:
            if self.text_widget.tag_ranges(tk.SEL):
                start, end = self.text_widget.tag_ranges(tk.SEL)
                selected_text = self.text_widget.get(start, end)
                
                # Eliminar timestamps completos y parciales, y espacios iniciales
                cleaned_text = re.sub(r'(\[.*?\]|\[.*?$|^.*?\]|\s{20})', '', selected_text, flags=re.MULTILINE)
                
                # Reemplazar múltiples espacios iniciales con un solo espacio
                cleaned_text = re.sub(r'^\s+', ' ', cleaned_text, flags=re.MULTILINE)
                
                # Reemplazar saltos de línea con espacios
                cleaned_text = re.sub(r'\n+', ' ', cleaned_text.strip())

                cleaned_text = re.sub(r'  ', ' ', cleaned_text.strip())
                
                self.clipboard_clear()
                self.clipboard_append(cleaned_text)
            return "break"
        except tk.TclError:
            pass

if __name__ == '__main__':
    import sys
    if len(sys.argv) > 1:
        file_path = sys.argv[1]
        app = TimestampNotepad(file_path)
    else:
        app = TimestampNotepad()
    app.mainloop()