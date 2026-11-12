function extractedStruct = extractColumnFromStruct(inputStruct, columnIndex)
    % extractColumnFromStruct Extracts a specified column from each field of a struct array.
    %
    % This function iterates through each field of a struct array and extracts 
    % the specified column from each field, assuming the fields are matrices or arrays.
    % The extracted column is then stored in a new struct array with the same field names.
    %
    % Inputs:
    %   inputStruct - A struct array from which the column will be extracted.
    %   columnIndex - The index of the column to be extracted from each field.
    %
    % Outputs:
    %   extractedStruct - A new struct array containing only the specified column 
    %                     from each field of the original struct array.
    %
    % Example:
    %   If 'inputStruct' has fields 'A', 'B', each being an array, then calling
    %   extractColumnFromStruct(inputStruct, 2) will extract the 2nd column from 
    %   'A' and 'B' for each element of the struct array.

    % Initialize the output struct
    extractedStruct = struct();
    
    % Get all field names from the input struct
    fieldNames = fieldnames(inputStruct);

    % Iterate over each element of the input struct
    for i = 1:length(inputStruct)
        % Iterate over each field
        for j = 1:length(fieldNames)
            currentFieldName = fieldNames{j};  % Current field name
            
            % Check if the field is not empty
            if ~isempty(inputStruct(i).(currentFieldName))
                % Assign the specified column of the field from inputStruct(i) to extractedStruct(i)
                extractedStruct(i).(currentFieldName) = inputStruct(i).(currentFieldName)(:, columnIndex);
            else
                % Handle the empty case
                extractedStruct(i).(currentFieldName) = [];  % Assign an empty array or another default value
            end
        end
    end
end
