use std::{error::Error, ffi::CString, fs::File, io::Read, path::PathBuf};

use windows::Win32::Graphics::{
    Direct3D::{ID3DBlob, ID3DInclude},
    Hlsl::D3D_COMPILE_STANDARD_FILE_INCLUDE,
};

const D3DCOMPILE_STANDARD_FILE_INCLUDE: &ID3DInclude = unsafe {
    std::mem::transmute::<_, &ID3DInclude>(&(D3D_COMPILE_STANDARD_FILE_INCLUDE as usize))
};

// Option<ID3DBlob>
pub fn Compile(input_file: &PathBuf) -> Result<(), Box<dyn Error>> {
    let input_data = {
        let err_func = |err| {
            eprintln!("failed to open file: {}", input_file.display());
            err
        };
        let mut file = match File::open(&input_file) {
            Ok(file) => file,
            Err(err) => return Err(err_func(Box::new(err))),
        };
        let mut data = match file.metadata() {
            Ok(meta) => Vec::with_capacity(meta.len() as usize),
            Err(err) => Vec::new(),
        };

        match file.read_to_end(&mut data) {
            Ok(_) => data,
            Err(err) => return Err(err_func(Box::new(err))),
        }
    };
    let file_name = CString::new(input_file.as_path().as_os_str().as_encoded_bytes())?;
    println!("file_name: {:?}", file_name);
    Ok(())
    // let file_name = CString::new(self.input_file).unwrap();
    // let model = CString::new(self.model).unwrap();

    // let mut output: CompileOutput = Default::default();

    // eprintln!("Calling D3DCompile2(");
    // eprintln!("\t{:p},", input_data.as_ptr());
    // eprintln!("\t{},", input_data.len());
    // eprintln!("\t{},", file_name.to_str().unwrap());
    // eprintln!("\t{:p},", self.d3d_defines.as_ptr());
    // eprintln!("\tD3D_COMPILE_STANDARD_FILE_INCLUDE,");
    // eprintln!("\t{},", self.entry_point.to_str().unwrap());
    // eprintln!("\t{},", model.to_str().unwrap());
    // eprintln!("\t0,");
    // eprintln!("\t0,");
    // eprintln!("\t0,");
    // eprintln!("\tNULL,");
    // eprintln!("\t0,");
    // eprintln!("\t{:p},", data.as_mut_ptr());
    // eprintln!("\t{:p})", errors.as_mut_ptr());

    // let hr = unsafe {
    //     D3DCompile2(
    //         input_data.as_ptr() as *const c_void,
    //         input_data.len(),
    //         PCSTR(file_name.as_bytes_with_nul().as_ptr() as *const u8),
    //         Some(self.d3d_defines.as_ptr()),
    //         D3DCOMPILE_STANDARD_FILE_INCLUDE,
    //         PCSTR(self.entry_point.as_bytes_with_nul().as_ptr()),
    //         PCSTR(model.as_bytes_with_nul().as_ptr()),
    //         self.flags1,
    //         0,
    //         0,
    //         None,
    //         0,
    //         &mut output.data,
    //         Some(&mut output.errors),
    //     )
    // };
    // (hr, output)
}
