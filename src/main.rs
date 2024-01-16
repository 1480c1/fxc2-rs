/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::{
    char, env, error,
    ffi::{c_void, CStr, CString},
    fs::File,
    io::Write,
    mem::MaybeUninit,
    process::{exit, ExitCode},
    slice,
};

use windows::{
    core::{PCSTR, PCWSTR},
    Win32::Graphics::{
        Direct3D::{Fxc::D3DCompileFromFile, ID3DBlob, ID3DInclude, D3D_SHADER_MACRO},
        Hlsl::D3D_COMPILE_STANDARD_FILE_INCLUDE,
    },
};

struct ProfilePrefix {
    name: &'static str,
    prefix: &'static str,
}

static PROFILE_PREFIX_TABLE: [ProfilePrefix; 12] = [
    ProfilePrefix {
        name: "ps_2_0",
        prefix: "g_ps20",
    },
    ProfilePrefix {
        name: "ps_2_a",
        prefix: "g_ps21",
    },
    ProfilePrefix {
        name: "ps_2_b",
        prefix: "g_ps21",
    },
    ProfilePrefix {
        name: "ps_2_sw",
        prefix: "g_ps2ff",
    },
    ProfilePrefix {
        name: "ps_3_0",
        prefix: "g_ps30",
    },
    ProfilePrefix {
        name: "ps_3_sw",
        prefix: "g_ps3ff",
    },
    ProfilePrefix {
        name: "vs_1_1",
        prefix: "g_vs11",
    },
    ProfilePrefix {
        name: "vs_2_0",
        prefix: "g_vs20",
    },
    ProfilePrefix {
        name: "vs_2_a",
        prefix: "g_vs21",
    },
    ProfilePrefix {
        name: "vs_2_sw",
        prefix: "g_vs2ff",
    },
    ProfilePrefix {
        name: "vs_3_0",
        prefix: "g_vs30",
    },
    ProfilePrefix {
        name: "vs_3_sw",
        prefix: "g_vs3ff",
    },
];

fn print_usage_arg() -> ExitCode {
    eprintln!("You have specified an argument that is not handled by fxc2");
    eprintln!("This isn't a sign of disaster, odds are it will be very easy to add support for this argument.");
    eprintln!(
        "Review the meaning of the argument in the real fxc program, and then add it into fxc2."
    );
    ExitCode::FAILURE
}

fn print_usage_missing(arg: &str) -> ExitCode {
    eprintln!("fxc2 is missing the {arg} argument. We expected to receive this, and it's likely things will nmot work correctly without it. Review fxc2 and make sure things will work.");
    ExitCode::FAILURE
}

fn print_usage_toomany() -> ExitCode {
    eprintln!("You specified multiple input files. We did not expect to receive this, and aren't prepared to handle multiple input files. You'll have to edit the source to behave the way you want.");
    ExitCode::FAILURE
}

struct ParseOpt {
    args: Vec<String>,
}

impl ParseOpt {
    fn new(args: Vec<String>) -> ParseOpt {
        ParseOpt { args }
    }
    fn end(&self) -> bool {
        self.args.len() == 0
    }
    fn get(&mut self) -> Option<String> {
        if self.end() {
            None
        } else {
            Some(self.args.remove(0))
        }
    }

    fn find_index(&self, option: &str) -> Option<usize> {
        self.args.iter().position(|i: &String| {
            let mut chars = i.chars();
            let first: Option<char> = chars.next();
            if i.len() == 1 || (first != Some('-') && first != Some('/')) {
                false
            } else {
                // fxc allows attached arguments, so we want to check if the option is a prefix of the argument
                let arg: String = chars.take(option.chars().count()).collect();
                arg == option
            }
        })
    }

    fn parse_one(&mut self, option: &str) -> Option<String> {
        if let Some(index) = self.find_index(option) {
            Some(self.args.remove(index))
        } else {
            None
        }
    }
    /// Parse an option that takes one argument
    /// Returns true if the option was found and removed
    /// If the option was found, the argument is returned in the argument parameter
    /// If the option was not found, the argument parameter is cleared
    /// The argument can take the form of `/<option><argument>` or `/<option> <argument>`
    fn parse_arg(&mut self, option: &str) -> Option<String> {
        let index = {
            if let Some(index) = self.find_index(option) {
                index
            } else {
                return None;
            }
        };
        let opt_len = option.chars().count() + 1; // +1 for the '-' or '/'

        // test to see if the argument is attached to the option
        let arg = self.args[index].clone();
        let arg_len = arg.chars().count();

        if arg_len > opt_len {
            self.args.remove(index);
            Some(arg.chars().skip(opt_len).collect())
        } else if index + 1 < self.args.len() {
            // test to see if the argument is a separate argument
            let arg = self.args[index + 1].clone();
            self.args.remove(index);
            self.args.remove(index);
            Some(arg)
        } else {
            None
        }
    }
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<String>>();
    let mut args = ParseOpt::new(args);

    if args.parse_one("?").is_some() || args.parse_one("help").is_some() {
        return print_usage_arg();
    }

    args.parse_one("nologo");
    let mut model = {
        if let Some(model) = args.parse_arg("T") {
            CString::new(model).expect("Failed to parse model")
        } else {
            return print_usage_missing("model");
        }
    };
    let mut entry_point = {
        if let Some(entry_point) = args.parse_arg("E") {
            CString::new(entry_point).expect("Failed to parse entry point")
        } else {
            return print_usage_missing("entryPoint");
        }
    };
    let mut variable_name = args.parse_arg("Vn");
    let mut output_file = {
        if let Some(output_file) = args.parse_arg("Fh") {
            output_file
        } else {
            return print_usage_missing("outputFile");
        }
    };
    if let Some(arg) = args.parse_one("Vi") {
        println!("option {arg} (Output include process details) acknowledged but ignored");
    }
    let mut defines: Vec<(CString, CString)> = Vec::new();
    while let Some(arg) = args.parse_arg("D") {
        // We can't construct D3D_SHADER_MACRO directly due to lifetime issues
        // store the defines for now and construct them after
        let mut define = arg.split('=');
        let name: CString =
            CString::new(define.next().unwrap()).expect("Failed to parse define name");
        let value: CString =
            CString::new(define.next().unwrap_or("1")).expect("Failed to parse define value");
        defines.push((name, value));
    }
    // Now that we have the strings stored, we can construct the D3D_SHADER_MACRO array
    let mut d3d_defines: Vec<D3D_SHADER_MACRO> = Vec::new();
    for (name, value) in defines.iter() {
        let mut define = D3D_SHADER_MACRO::default();
        define.Name = PCSTR(name.as_bytes_with_nul().as_ptr());
        define.Definition = PCSTR(value.as_bytes_with_nul().as_ptr());
        d3d_defines.push(define);
    }
    d3d_defines.push(D3D_SHADER_MACRO::default()); // null terminator

    let input_file = {
        if let Some(input_file) = args.get() {
            input_file.encode_utf16().collect::<Vec<u16>>()
        } else {
            return print_usage_missing("inputFile");
        }
    };

    if !args.end() {
        eprintln!("fxc2: Unhandled arguments:");
        while let Some(arg) = args.get() {
            eprintln!("  {}", arg);
        }
        return print_usage_toomany();
    }

    // Default output variable name
    if variable_name.is_none() {
        let model = model.to_str().unwrap();
        for i in PROFILE_PREFIX_TABLE.iter() {
            if i.name == model {
                variable_name = Some(format!("{}_{}", i.prefix, entry_point.to_str().unwrap()));
                break;
            }
        }
    }
    // if the model doesn't match any from our table, use g_ as the prefix
    if variable_name.is_none() {
        variable_name = Some(format!("g_{}", entry_point.to_str().unwrap()));
    }
    let variable_name = variable_name.unwrap();

    eprintln!("option -T (Shader Model/Profile) with arg '{:?}'", model);
    eprintln!("option -E (Entry Point) with arg '{:?}'", entry_point);
    eprintln!("option -Fh (Output File) with arg {output_file}");
    eprintln!("option -Vn (Variable Name) with arg '{variable_name}'");

    // ====================================================================================
    // Shader Compilation

    let mut output: MaybeUninit<Option<ID3DBlob>> = MaybeUninit::uninit();
    let mut errors: MaybeUninit<Option<ID3DBlob>> = MaybeUninit::uninit();

    let include: &ID3DInclude = unsafe {
        std::mem::transmute::<_, &ID3DInclude>(&(D3D_COMPILE_STANDARD_FILE_INCLUDE as usize))
    };

    eprintln!("Calling D3DCompileFromFile(");
    eprintln!("\t{},", String::from_utf16(&input_file).unwrap());
    eprintln!("\t{:?},", d3d_defines);
    eprintln!("\tD3D_COMPILE_STANDARD_FILE_INCLUDE,");
    eprintln!("\t{},", entry_point.to_str().unwrap());
    eprintln!("\t{},", model.to_str().unwrap());
    eprintln!("\t0,");
    eprintln!("\t0,");
    eprintln!("\t{:p},", output.as_mut_ptr());
    eprintln!("\t{:p})", errors.as_mut_ptr());

    let hr = unsafe {
        D3DCompileFromFile(
            PCWSTR(input_file.as_ptr()),
            Some(d3d_defines.as_ptr()),
            include,
            PCSTR(entry_point.as_bytes_with_nul().as_ptr()),
            PCSTR(model.as_bytes_with_nul().as_ptr()),
            0,
            0,
            output.as_mut_ptr(),
            Some(errors.as_mut_ptr()),
        )
    };

    let (output, errors) = unsafe { (output.assume_init(), errors.assume_init()) };

    if hr.is_err() {
        if let Some(errors) = errors {
            let error = unsafe { CStr::from_ptr(errors.GetBufferPointer() as *const i8) };
            eprintln!("Got an error while compiling:");
            eprintln!("{}", error.to_string_lossy());
        } else {
            eprintln!("Got an error while compiling, but no error message from the function");
        }
        return ExitCode::FAILURE;
    }

    let output = output.unwrap();

    let data = unsafe {
        let out_string = output.GetBufferPointer() as *const u8;
        let len = output.GetBufferSize();
        println!("Output length: {}", len);
        let mut data = Vec::with_capacity(len);
        std::ptr::copy(out_string, data.as_mut_ptr(), len);
        data.set_len(len);
        data
    };

    let mut file = File::create(output_file.clone()).expect("Failed to create output file");

    write!(file, "const BYTE {variable_name}[] =\n{{\n").unwrap();
    for (i, byte) in data.iter().enumerate() {
        let byte = *byte as i8;
        write!(file, "{:4}", byte).unwrap();
        if i != data.len() - 1 {
            write!(file, ",").unwrap();
        }
        if i % 6 == 5 {
            write!(file, "\n").unwrap();
        }
    }
    write!(file, "\n}};").unwrap();
    drop(file);

    println!(
        "Wrote {} bytes of shader output to {}",
        data.len(),
        output_file
    );

    ExitCode::SUCCESS
}
