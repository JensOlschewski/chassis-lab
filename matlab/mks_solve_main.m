function mks_solve_main(modelJson, outJson, designJson)
try
    if nargin >= 3 && ~isempty(designJson)
        history = run_mks_solver(modelJson, jsondecode(fileread(designJson)));
    else
        history = run_mks_solver(modelJson);
    end
    fid = fopen(outJson, 'w');
    fwrite(fid, jsonencode(history));
    fclose(fid);
catch err
    fprintf(2, '%s\n%s\n', err.identifier, err.message);
    exit(1);
end
end
