function validateJsonStruct(jsonStruct)
    % This function checks if all entries in Hardpoints, Bushings, and Euler Angles are numeric.
    % If any non-numeric values are found, it raises an error.

    % Check Hardpoints
    fields = fieldnames(jsonStruct.Hardpoints);
    for i = 1:numel(fields)
        fieldData = jsonStruct.Hardpoints.(fields{i});
        if ~isnumeric(fieldData)
            error(['Non-numeric value found in Hardpoints field: ', fields{i}]);
        end
    end

    % Check Bushings
    bushingTypes = {'primary_bushings', 'secondary_bushings', 'additional_bushings'};
    for b = 1:numel(bushingTypes)
        bushingType = bushingTypes{b};
        if isfield(jsonStruct, bushingType)
            bushingNames = fieldnames(jsonStruct.(bushingType));
            for bn = 1:numel(bushingNames)
                bushingName = bushingNames{bn};

                % Check 'stiffness' field
                if isfield(jsonStruct.(bushingType).(bushingName), 'stiffness')
                    stiffnessData = jsonStruct.(bushingType).(bushingName).stiffness;

                    if isnumeric(stiffnessData)
                        % Numeric value, valid
                        % Do nothing
                    elseif ischar(stiffnessData) && endsWith(stiffnessData, '.json', 'IgnoreCase', true)
                        % Valid .json file path
                        % Do nothing
                    else
                        % Invalid value
                        error(['Invalid value found in stiffness field of Bushing: ', bushingName]);
                    end
                end

                % Check Euler Angles
                sides = {'i', 'j'};
                for side = 1:numel(sides)
                    sideName = sides{side};
                    if isfield(jsonStruct.(bushingType).(bushingName).(sideName), 'orientation') && ...
                       isfield(jsonStruct.(bushingType).(bushingName).(sideName).orientation, 'euler_angles')
                        eulerData = jsonStruct.(bushingType).(bushingName).(sideName).orientation.euler_angles;
                        if ~isnumeric(eulerData)
                            error(['Non-numeric value found in Euler Angles of Bushing: ', bushingName, ', Side: ', sideName]);
                        end
                    end
                end
            end
        end
    end
end
