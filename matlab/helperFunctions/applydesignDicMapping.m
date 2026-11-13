function jsonStruct = applydesignDicMapping(jsonStruct, designDic)
    % applydesignDicMapping Replaces design variable names in jsonStruct by their values.
    %
    % Inputs:
    %   - jsonStruct : The original JSON structure to be updated
    %   - designDic  : dictionary (string -> cell) from variable name to value. A value is a
    %                  number, or a text for a stiffness characteristic file.
    %
    % Where a name may stand (anywhere a number is expected, inside arrays too):
    %   - Hardpoints                      every coordinate
    %   - bushings and additional_forces  active, stiffness (also a whole characteristic
    %                                     file path), free_length
    %   - bushing sides                   orientation euler_angles and helpvec
    %   - sim_param.step_parameters       t_start, t_end, h
    %   - drives                          names inside the signal expressions, e.g.
    %                                     "AMP*sin(2*pi*TIME/30)"
    %
    % A text ending in .json is always a file path, never a variable name.

    usedKeys = strings(0);

    for name = string(fieldnames(jsonStruct.Hardpoints))'
        jsonStruct.Hardpoints.(name) = resolve(jsonStruct.Hardpoints.(name), false);
    end

    groups = ["primary_bushings", "secondary_bushings", "additional_bushings", "additional_forces"];
    for group = groups(isfield(jsonStruct, groups))
        for name = string(fieldnames(jsonStruct.(group)))'
            connector = jsonStruct.(group).(name);
            for property = ["active", "stiffness", "free_length"]
                if isfield(connector, property)
                    connector.(property) = resolve(connector.(property), property == "stiffness");
                end
            end
            for side = ["i", "j"]
                if isfield(connector, side) && isfield(connector.(side), 'orientation')
                    for property = ["euler_angles", "helpvec"]
                        if isfield(connector.(side).orientation, property)
                            connector.(side).orientation.(property) = resolve(connector.(side).orientation.(property), false);
                        end
                    end
                end
            end
            jsonStruct.(group).(name) = connector;
        end
    end

    for name = ["t_start", "t_end", "h"]
        jsonStruct.sim_param.step_parameters.(name) = resolve(jsonStruct.sim_param.step_parameters.(name), false);
    end

    for group = ["force_torque_drives", "relonomic_drives"]
        for name = string(fieldnames(jsonStruct.drives.(group)))'
            signal = jsonStruct.drives.(group).(name).signal;
            if iscell(signal)
                jsonStruct.drives.(group).(name).signal = cellfun(@substitute, signal, 'UniformOutput', false);
            end
        end
    end

    unused = setdiff(keys(designDic), usedKeys, 'stable');
    for k = 1:numel(unused)
        warning('Design variable %s was not used in the JSON structure.', unused(k));
    end

    function value = resolve(value, allowText)
        % Replaces a variable name, or the names inside an array, by their values.
        if ischar(value) || isstring(value)
            if ~endsWith(value, '.json', 'IgnoreCase', true)
                value = lookup(value, allowText);
            end
        elseif iscell(value)
            for k = 1:numel(value)
                if ischar(value{k})
                    value{k} = lookup(value{k}, false);
                end
            end
            if all(cellfun(@isnumeric, value))
                value = cell2mat(value);
            end
        elseif ~isnumeric(value) && ~islogical(value)
            error('Unsupported data type for a design variable field.');
        end
    end

    function expression = substitute(expression)
        % Replaces design variable names inside a drive signal expression by their numbers.
        if ~ischar(expression)
            return
        end
        for name = keys(designDic)'
            pattern = "\<" + regexptranslate('escape', name) + "\>";
            if ~isempty(regexp(expression, pattern, 'once'))
                expression = regexprep(expression, pattern, "(" + num2str(lookup(name, false), 17) + ")");
            end
        end
    end

    function value = lookup(name, allowText)
        if ~isKey(designDic, name)
            error('Design variable %s not found in designDic.', name);
        end
        entry = designDic(name);
        value = entry{1};
        if ~allowText && ischar(value)
            error('Design variable %s must be a number here.', name);
        end
        usedKeys(end+1) = string(name);
    end
end
