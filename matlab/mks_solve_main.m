function mks_solve_main(modelJson, outJson, varargin)
% mks_solver <model.json> <out.json> [--design FILE]
try
    design = [];
    for k = 1:2:numel(varargin)
        switch varargin{k}
            case '--design'
                design = readDesign(varargin{k+1});
            otherwise
                error('mks:args', 'Unknown option %s', varargin{k});
        end
    end
    history = run_mks_solver(modelJson, design);
    fid = fopen(outJson, 'w');
    fwrite(fid, jsonencode(history));
    fclose(fid);
catch err
    fprintf(2, '%s\n%s\n', err.identifier, err.message);
    exit(1);
end
end

function design = readDesign(file)
s = jsondecode(fileread(file));
names = string(fieldnames(s));
design = configureDictionary("string", "cell");
for n = names'
    value = s.(n);
    if ~ischar(value)
        value = double(value);
    end
    design(n) = {value};
end
end
