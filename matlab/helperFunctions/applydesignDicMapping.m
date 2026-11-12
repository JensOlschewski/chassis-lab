function updatedjsonStruct = applydesignDicMapping(jsonStruct, designDic)
    % applydesignDicMapping Updates the jsonStruct with values from designDic
    %
    % Inputs:
    %   - jsonStruct : The original JSON structure to be updated
    %   - designDic  : MATLAB dictionary with keys as strings indicating the parameter to update
    %
    % Output:
    %   - updatedjsonStruct : The updated JSON structure

    % List of used keys
    maxKeys = numel(keys(designDic));
    usedKeys = cell(1, maxKeys);
    usedKeyIndex = 0;

    % Process Hardpoints
    fields = fieldnames(jsonStruct.Hardpoints);
    for i = 1:numel(fields)
        fieldData = jsonStruct.Hardpoints.(fields{i});
        if iscell(fieldData)
            for j = 1:numel(fieldData)
                if ischar(fieldData{j})  % Check if the entry is a design variable (string)
                    varName = fieldData{j};
                    if isKey(designDic, varName)
                        jsonStruct.Hardpoints.(fields{i}){j} = designDic(varName);  % Replace with value from designDic
                        usedKeyIndex = usedKeyIndex + 1;
                        usedKeys{usedKeyIndex} = varName;
                        if all(cellfun(@isnumeric, jsonStruct.Hardpoints.(fields{i})))
                            jsonStruct.Hardpoints.(fields{i}) = cell2mat(jsonStruct.Hardpoints.(fields{i}));
                        end
                    else
                        error(['Design variable ', varName, ' not found in designDic.']);
                    end
                end
            end
        elseif isnumeric(fieldData)
            continue;
        else
            error('Unsupported data type for Hardpoints field.');
        end
    end
    % Process stiffness and euler fields
    bushingTypes = {"primary_bushings", "secondary_bushings", "additional_bushings"};
    for b = 1:numel(bushingTypes)
        bushingType = bushingTypes{b};
        if isfield(jsonStruct, bushingType)
            bushingNames = fieldnames(jsonStruct.(bushingType));
            for bn = 1:numel(bushingNames)
                bushingName = bushingNames{bn};

                % Check for stiffness field and update
                if isfield(jsonStruct.(bushingType).(bushingName), 'stiffness')
                    stiffnessArray = jsonStruct.(bushingType).(bushingName).stiffness;
                    for s = 1:numel(stiffnessArray)
                        if iscell(stiffnessArray)
                            if ischar(stiffnessArray{s})
                                varName = stiffnessArray{s};
                                if isKey(designDic, varName)
                                    jsonStruct.(bushingType).(bushingName).stiffness{s} = designDic(varName);
                                    usedKeyIndex = usedKeyIndex + 1;
                                    usedKeys{usedKeyIndex} = varName;
                                    if all(cellfun(@isnumeric, jsonStruct.(bushingType).(bushingName).stiffness))
                                        jsonStruct.(bushingType).(bushingName).stiffness = cell2mat(jsonStruct.(bushingType).(bushingName).stiffness);
                                    end
                                else
                                    error(['Design variable ', varName, ' not found in designDic.']);
                                end
                            end
                        elseif isnumeric(stiffnessArray) || ischar(stiffnessArray)
                            continue;
                        else
                            error('Unsupported data type for Stiffness field at %s.', bushingName);
                        end
                    end
                end

                % Check for eulerAngles field and update
                sides = {"i","j"};
                for side = 1:numel(sides)
                    sideName = sides{side};
                    if isfield(jsonStruct.(bushingType).(bushingName).(sideName).orientation, 'euler_angles')
                        eulerArray = jsonStruct.(bushingType).(bushingName).(sideName).orientation.euler_angles;
                        for e = 1:numel(eulerArray)
                            if iscell(eulerArray)
                                if ischar(eulerArray{e})
                                    varName = eulerArray{e};
                                    if isKey(designDic, varName)
                                        jsonStruct.(bushingType).(bushingName).(sideName).orientation.euler_angles{e} = designDic(varName);
                                        usedKeyIndex = usedKeyIndex + 1;
                                        usedKeys{usedKeyIndex} = varName;
                                        if all(cellfun(@isnumeric, jsonStruct.(bushingType).(bushingName).(sideName).orientation.euler_angles))
                                            jsonStruct.(bushingType).(bushingName).(sideName).orientation.euler_angles = cell2mat(jsonStruct.(bushingType).(bushingName).(sideName).orientation.euler_angles);
                                        end
                                    else
                                        error(['Design variable ', varName, ' not found in designDic.']);
                                    end
                                end
                            elseif isnumeric(eulerArray)
                                continue;
                            else
                                error('Unsupported data type for Euler field.');
                            end
                        end

                    end
                end

                % Check for activity toogle
                if isfield(jsonStruct.(bushingType).(bushingName), 'active')
                    isActive = jsonStruct.(bushingType).(bushingName).active;
                    if ischar(isActive)
                        varName = isActive;
                        if isKey(designDic, varName)
                            jsonStruct.(bushingType).(bushingName).active = designDic(varName);
                            usedKeyIndex = usedKeyIndex + 1;
                            usedKeys{usedKeyIndex} = varName;
                        else
                            error(['Design variable ', varName, ' not found in designDic.']);
                        end
                    elseif isnumeric(isActive)
                        continue;
                    else
                        error('Unsupported data type for Stiffness field at %s.', bushingName);
                    end
                end
            end
        end
    end

    % Trim the usedKeys array to the actual number of used keys
    usedKeys = usedKeys(1:usedKeyIndex);

    % Final check: Warning for unused variables in the designDic
    dicKeys = keys(designDic);  % Get all keys from the dictionary
    unusedKeys = setdiff(dicKeys, usedKeys, 'stable');  % Find unused keys while preserving the original order

    % Issue a warning for each unused design variable
    for i = 1:numel(unusedKeys)
        warning(['Design variable ', unusedKeys{i}, ' was not used in the JSON structure.']);
    end

    updatedjsonStruct = jsonStruct;
end
